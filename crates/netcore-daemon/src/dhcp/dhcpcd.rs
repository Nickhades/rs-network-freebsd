//! dhcpcd backend.
//!
//! dhcpcd 10 on FreeBSD exposes a per-interface control socket in
//! /var/run/dhcpcd/.  The requested JSON listen command is sent on a locally
//! bound datagram socket.  If a particular dhcpcd build does not speak JSON,
//! the backend still observes the interface address and the lease file; this
//! keeps the daemon functional without silently depending on a private socket
//! protocol.  Externally started dhcpcd instances are never killed.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::net::IpAddr;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use super::lease::{LeaseDetails, lease_for_interface};
use super::{
    DhcpClient, DhcpEvent, Error, EventStream, Shared, dhcpcd_control_paths, dhcpcd_lease_paths,
    shared_emit, shared_mark_external, shared_reset, shared_subscribe, write_resolv_conf,
};
use crate::system::validate_ifname;

static SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);

struct Runtime {
    stop: Arc<AtomicBool>,
    child: Option<Arc<Mutex<Child>>>,
    monitor: JoinHandle<()>,
    control: Option<JoinHandle<()>>,
    external: bool,
}

/// One dhcpcd process per interface.
pub(crate) struct DhcpcdBackend {
    dhcpcd: PathBuf,
    shared: Arc<Mutex<Shared>>,
    runtimes: Mutex<HashMap<String, Runtime>>,
    dns: DnsState,
}

#[derive(Clone)]
struct DnsState {
    path: PathBuf,
    servers: Arc<Mutex<HashMap<String, Vec<IpAddr>>>>,
}

impl DnsState {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            servers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Update the daemon-owned resolv.conf.  External dhcpcd instances keep
    /// ownership of DNS, so a false manage flag is a no-op.
    fn apply(&self, iface: &str, event: &DhcpEvent, manage: bool) {
        if !manage {
            return;
        }
        let mut servers = self.servers.lock().unwrap_or_else(PoisonError::into_inner);
        match event {
            DhcpEvent::Bound { dns, .. } => {
                if !dns.is_empty() {
                    servers.insert(iface.to_owned(), dns.clone());
                }
            }
            DhcpEvent::Renew { .. } => {}
            DhcpEvent::Expired | DhcpEvent::Stopped => {
                servers.remove(iface);
            }
        }
        let mut merged = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for list in servers.values() {
            for server in list {
                if seen.insert(*server) {
                    merged.push(*server);
                }
            }
        }
        if let Err(error) = write_resolv_conf(&self.path, &merged) {
            tracing::warn!(path = %self.path.display(), %error, "写入 resolv.conf 失败");
        }
    }
}

impl DhcpcdBackend {
    pub(crate) fn new(dhcpcd: PathBuf, resolv_conf: PathBuf) -> Self {
        Self {
            dhcpcd,
            shared: Arc::new(Mutex::new(Shared::new())),
            runtimes: Mutex::new(HashMap::new()),
            dns: DnsState::new(resolv_conf),
        }
    }
}

impl DhcpClient for DhcpcdBackend {
    fn start(&self, iface: &str) -> Result<(), Error> {
        validate_ifname(iface)?;
        let mut runtimes = self.runtimes.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(runtime) = runtimes.get(iface) {
            if runtime.external {
                return Ok(());
            }
            // -b daemonizes, so the launcher having exited is normal.  A live
            // control socket means the backend is already running.
            if dhcpcd_control_paths(iface).iter().any(|path| path.exists()) {
                return Ok(());
            }
            if let Some(child) = &runtime.child {
                let mut child = child.lock().unwrap_or_else(PoisonError::into_inner);
                if child.try_wait()?.is_none() {
                    return Ok(());
                }
            }
            if let Some(dead) = runtimes.remove(iface) {
                dead.stop.store(true, Ordering::SeqCst);
                let _ = dead.monitor.join();
                if let Some(control) = dead.control {
                    let _ = control.join();
                }
            }
        }

        shared_reset(&self.shared, iface);
        let external = running_external_dhcpcd(iface);
        let child = if external.is_some() {
            shared_mark_external(&self.shared, iface, true);
            None
        } else {
            shared_mark_external(&self.shared, iface, false);
            Some(self.spawn(iface)?)
        };
        let stop = Arc::new(AtomicBool::new(false));
        let manage_dns = external.is_none();
        let monitor = spawn_monitor(
            iface.to_owned(),
            child.clone(),
            Arc::clone(&stop),
            Arc::clone(&self.shared),
            self.dns.clone(),
            manage_dns,
        );
        let control = child.as_ref().map(|_| {
            spawn_control_listener(
                iface.to_owned(),
                Arc::clone(&stop),
                Arc::clone(&self.shared),
                self.dns.clone(),
                manage_dns,
            )
        });
        runtimes.insert(
            iface.to_owned(),
            Runtime {
                stop,
                child,
                monitor,
                control,
                external: external.is_some(),
            },
        );
        if let Some(pid) = external {
            tracing::info!(ifname = iface, pid, "检测到已运行的 dhcpcd，沿用其租约");
        }
        Ok(())
    }

    fn stop(&self, iface: &str) -> Result<(), Error> {
        let runtime = self
            .runtimes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(iface);
        let Some(runtime) = runtime else {
            return Ok(());
        };
        runtime.stop.store(true, Ordering::SeqCst);
        if !runtime.external {
            let socket_stopped = send_control(iface, "{\"command\":\"terminate\"}\\n");
            if !socket_stopped {
                release(&self.dhcpcd, iface);
            }
            if let Some(child) = &runtime.child {
                terminate(child);
            }
            self.dns.apply(iface, &DhcpEvent::Stopped, true);
        }
        shared_mark_external(&self.shared, iface, false);
        shared_emit(&self.shared, iface, DhcpEvent::Stopped);
        let _ = runtime.monitor.join();
        if let Some(control) = runtime.control {
            let _ = control.join();
        }
        Ok(())
    }

    fn subscribe(&self, iface: &str) -> Box<dyn EventStream> {
        shared_subscribe(&self.shared, iface)
    }

    fn was_external(&self, iface: &str) -> bool {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_external(iface)
    }

    fn shutdown(&self) {
        let names: Vec<String> = self
            .runtimes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .cloned()
            .collect();
        for name in names {
            let _ = self.stop(&name);
        }
    }
}

impl DhcpcdBackend {
    fn spawn(&self, iface: &str) -> Result<Arc<Mutex<Child>>> {
        let mut command = Command::new(&self.dhcpcd);
        // -B is important: dhcpcd must stay in the foreground so this daemon
        // owns its lifetime.  The requested control group limits access to the
        // socket on versions that support it.
        command
            .arg("-q")
            .arg("-b")
            .arg("-C")
            .arg("resolv.conf")
            .arg("--controlgroup")
            .arg("wheel")
            .arg("--metric")
            .arg("100")
            .arg(iface)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().with_context(|| {
            format!(
                "启动 {} -q -b -C resolv.conf --controlgroup wheel --metric 100 {iface} 失败",
                self.dhcpcd.display()
            )
        })?;
        if let Some(stdout) = child.stdout.take() {
            pipe_to_log(stdout, "dhcpcd stdout");
        }
        if let Some(stderr) = child.stderr.take() {
            pipe_to_log(stderr, "dhcpcd stderr");
        }
        tracing::info!(ifname = iface, pid = child.id(), "已启动 dhcpcd");
        Ok(Arc::new(Mutex::new(child)))
    }
}

fn pipe_to_log<R: Read + Send + 'static>(reader: R, label: &'static str) {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(|line| line.ok()) {
            tracing::debug!(target: "netcore_daemon::dhcpcd", "{label}: {line}");
        }
    });
}

fn spawn_monitor(
    iface: String,
    child: Option<Arc<Mutex<Child>>>,
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
    dns: DnsState,
    manage_dns: bool,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut bound = false;
        let mut last: Option<LeaseDetails> = None;
        let mut launcher_exit_logged = false;
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            // dhcpcd -b daemonizes; the spawned launcher exiting is normal and
            // must not be treated as a stopped DHCP client.  The control
            // socket and the lease are the authority.
            if !launcher_exit_logged
                && let Some(child) = &child
                && let Ok(Some(status)) = child
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .try_wait()
            {
                tracing::debug!(ifname = iface, %status, "dhcpcd launcher 已退出（后台模式）");
                launcher_exit_logged = true;
            }
            let lease = lease_for_interface(&iface, &dhcpcd_lease_paths(&iface));
            match lease {
                Some(lease) => {
                    if !bound {
                        let event = DhcpEvent::Bound {
                            ip: lease.ip,
                            subnet: lease.subnet,
                            gateway: lease.gateway,
                            dns: lease.dns.clone(),
                        };
                        dns.apply(&iface, &event, manage_dns);
                        shared_emit(&shared, &iface, event);
                        bound = true;
                    } else if last.as_ref() != Some(&lease) {
                        let event = DhcpEvent::Renew { ip: lease.ip };
                        dns.apply(&iface, &event, manage_dns);
                        shared_emit(&shared, &iface, event);
                    }
                    last = Some(lease);
                }
                None if bound => {
                    let event = DhcpEvent::Expired;
                    dns.apply(&iface, &event, manage_dns);
                    shared_emit(&shared, &iface, event);
                    bound = false;
                    last = None;
                }
                None => {}
            }
            thread::sleep(Duration::from_millis(300));
        }
    })
}

fn spawn_control_listener(
    iface: String,
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
    dns: DnsState,
    manage_dns: bool,
) -> JoinHandle<()> {
    thread::spawn(move || {
        // The backgrounded launcher may take a moment to create the socket.
        let socket_path = loop {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            if let Some(path) = dhcpcd_control_paths(&iface)
                .into_iter()
                .find(|path| path.exists())
            {
                break path;
            }
            thread::sleep(Duration::from_millis(100));
        };
        let local = std::env::temp_dir().join(format!(
            "netcore-dhcpcd-{}-{}.sock",
            std::process::id(),
            SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&local);
        let socket = match UnixDatagram::bind(&local) {
            Ok(socket) => socket,
            Err(error) => {
                tracing::debug!(ifname = %iface, %error, "无法创建 dhcpcd 本地控制套接字");
                return;
            }
        };
        if let Err(error) = socket.connect(&socket_path) {
            tracing::debug!(ifname = %iface, %error, "无法连接 dhcpcd 控制套接字");
            let _ = std::fs::remove_file(&local);
            return;
        }
        if let Err(error) = socket.send(b"{\"command\":\"listen\"}\n") {
            tracing::debug!(ifname = %iface, %error, "发送 dhcpcd listen 失败");
        }
        let _ = socket.set_read_timeout(Some(Duration::from_secs(1)));
        let mut buffer = [0_u8; 8192];
        while !stop.load(Ordering::SeqCst) {
            match socket.recv(&mut buffer) {
                Ok(count) => {
                    for line in buffer[..count].split(|byte| *byte == b'\n') {
                        if let Ok(text) = std::str::from_utf8(line)
                            && let Some(event) = parse_control_event(text)
                        {
                            dns.apply(&iface, &event, manage_dns);
                            shared_emit(&shared, &iface, event);
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(error) => {
                    tracing::debug!(ifname = %iface, %error, "读取 dhcpcd 控制事件失败");
                    break;
                }
            }
        }
        let _ = std::fs::remove_file(&local);
    })
}

/// Parse one JSON event.  The parser also accepts a plain event name for
/// dhcpcd builds whose control protocol is textual instead of JSON.
#[must_use]
pub(crate) fn parse_control_event(line: &str) -> Option<DhcpEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return parse_plain_event(line);
    };
    let kind = value
        .get("event")
        .or_else(|| value.get("type"))
        .or_else(|| value.get("command"))
        .and_then(Value::as_str)?
        .to_ascii_uppercase();
    match kind.as_str() {
        "BOUND" | "BOUND4" | "CONNECTED" => {
            let ip = value
                .get("ip")
                .or_else(|| value.get("address"))
                .and_then(Value::as_str)
                .and_then(|text| text.parse::<IpAddr>().ok())?;
            let subnet = value
                .get("subnet")
                .or_else(|| value.get("prefix"))
                .and_then(parse_subnet)
                .unwrap_or(24);
            let gateway = value
                .get("gateway")
                .or_else(|| value.get("router"))
                .and_then(Value::as_str)
                .and_then(|text| text.parse::<IpAddr>().ok())
                .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
            let dns = value
                .get("dns")
                .or_else(|| value.get("domain_name_servers"))
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .filter_map(|text| text.parse::<IpAddr>().ok())
                        .collect()
                })
                .unwrap_or_default();
            Some(DhcpEvent::Bound {
                ip,
                subnet,
                gateway,
                dns,
            })
        }
        "RENEW" => Some(DhcpEvent::Renew {
            ip: value
                .get("ip")
                .or_else(|| value.get("address"))
                .and_then(Value::as_str)
                .and_then(|text| text.parse::<IpAddr>().ok())?,
        }),
        "EXPIRED" | "EXPIRE" | "LOST" => Some(DhcpEvent::Expired),
        "STOPPED" | "STOP" | "TERMINATED" => Some(DhcpEvent::Stopped),
        _ => None,
    }
}

fn parse_plain_event(line: &str) -> Option<DhcpEvent> {
    let upper = line.to_ascii_uppercase();
    // A textual BOUND/RENEW line without an address is not actionable and
    // must not be mistaken for a successful lease.  The lease/interface
    // fallback emits the real Bound event.
    if upper.contains("EXPIRE") {
        return Some(DhcpEvent::Expired);
    }
    if upper.contains("STOPPED") || upper.contains("TERMINATED") {
        return Some(DhcpEvent::Stopped);
    }
    None
}

fn parse_subnet(value: &Value) -> Option<u8> {
    if let Some(number) = value.as_u64() {
        return u8::try_from(number).ok().filter(|prefix| *prefix <= 32);
    }
    let text = value.as_str()?.trim();
    if let Ok(prefix) = text.parse::<u8>() {
        return (prefix <= 32).then_some(prefix);
    }
    let mask: std::net::Ipv4Addr = text.parse().ok()?;
    Some(u32::from(mask).count_ones() as u8)
}

fn send_control(iface: &str, payload: &str) -> bool {
    let Some(path) = dhcpcd_control_paths(iface)
        .into_iter()
        .find(|path| path.exists())
    else {
        return false;
    };
    let local = std::env::temp_dir().join(format!(
        "netcore-dhcpcd-stop-{}-{}.sock",
        std::process::id(),
        SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&local);
    let mut sent = false;
    if let Ok(socket) = UnixDatagram::bind(&local) {
        if socket.connect(&path).is_ok() {
            sent = socket.send(payload.as_bytes()).is_ok();
        }
        let _ = std::fs::remove_file(&local);
    }
    sent
}

fn release(dhcpcd: &Path, iface: &str) {
    let mut child = match Command::new(dhcpcd)
        .arg("-k")
        .arg(iface)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            tracing::debug!(ifname = iface, %error, "dhcpcd -k 回退失败");
            return;
        }
    };
    for _ in 0..20 {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => {
                tracing::debug!(ifname = iface, %error, "等待 dhcpcd -k 失败");
                return;
            }
        }
    }
    tracing::debug!(ifname = iface, "dhcpcd -k 超过 2 秒，强制结束");
    let _ = child.kill();
    let _ = child.wait();
}

fn terminate(child: &Arc<Mutex<Child>>) {
    let mut child = child.lock().unwrap_or_else(PoisonError::into_inner);
    let pid = child.id();
    // SAFETY: kill(2) with a pid we created and a valid signal.
    let rc = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGTERM) };
    if rc != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            tracing::warn!(pid, %error, "SIGTERM dhcpcd 失败");
        }
    }
    for _ in 0..50 {
        match child.try_wait() {
            Ok(Some(status)) => {
                tracing::debug!(pid, %status, "dhcpcd 已退出");
                return;
            }
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => {
                tracing::warn!(pid, %error, "等待 dhcpcd 失败");
                return;
            }
        }
    }
    tracing::warn!(pid, "dhcpcd 未在 5 秒内退出，发送 SIGKILL");
    let _ = child.kill();
    let _ = child.wait();
}

fn running_external_dhcpcd(ifname: &str) -> Option<u32> {
    // A system-wide dhcpcd may own every interface and expose only
    // /var/run/dhcpcd/pid.  Adopt it rather than starting a competing daemon.
    let global = std::fs::read_to_string("/var/run/dhcpcd/pid")
        .ok()
        .and_then(|text| text.trim().parse::<libc::pid_t>().ok())
        .filter(|pid| *pid > 0)
        .filter(|pid| unsafe { libc::kill(*pid, 0) } == 0);
    if global.is_some() {
        return global.map(|pid| pid as u32);
    }
    for path in [
        Path::new("/var/run/dhcpcd").join(format!("{ifname}.pid")),
        PathBuf::from(format!("/var/run/dhcpcd-{ifname}.pid")),
    ] {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(pid) = text.trim().parse::<libc::pid_t>() else {
            continue;
        };
        if pid <= 0 {
            continue;
        }
        // SAFETY: signal 0 only probes existence and permission.
        if unsafe { libc::kill(pid, 0) } == 0 {
            return Some(pid as u32);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn fake_dhcpcd_emits_bound_and_stops() {
        let path = std::env::temp_dir().join(format!("netcore-fake-dhcpcd-{}", std::process::id()));
        std::fs::write(
            &path,
            "#!/bin/sh\n[ \"$1\" = \"-k\" ] && exit 0\nexec sleep 30\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).unwrap();

        let resolv =
            std::env::temp_dir().join(format!("netcore-fake-resolv-{}", std::process::id()));
        let backend = DhcpcdBackend::new(path.clone(), resolv);
        let mut events = backend.subscribe("lo0");
        backend.start("lo0").expect("start fake dhcpcd");
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(events.next());
        });
        let event = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("bound event");
        assert!(matches!(event, Some(DhcpEvent::Bound { .. })));
        backend.stop("lo0").expect("stop fake dhcpcd");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn dns_state_writes_and_cleans_up() {
        let path = std::env::temp_dir().join(format!("netcore-dns-test-{}", std::process::id()));
        let state = DnsState::new(path.clone());
        let event = DhcpEvent::Bound {
            ip: "192.0.2.10".parse().unwrap(),
            subnet: 24,
            gateway: "192.0.2.1".parse().unwrap(),
            dns: vec!["192.0.2.53".parse().unwrap()],
        };
        state.apply("wlan0", &event, true);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("nameserver 192.0.2.53"));
        state.apply("wlan0", &DhcpEvent::Stopped, true);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("nameserver"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn parses_bound_json() {
        let event = parse_control_event(
            r#"{"event":"BOUND","ip":"192.0.2.10","subnet":24,"gateway":"192.0.2.1","dns":["192.0.2.53"]}"#,
        )
        .expect("event");
        assert_eq!(
            event,
            DhcpEvent::Bound {
                ip: "192.0.2.10".parse().unwrap(),
                subnet: 24,
                gateway: "192.0.2.1".parse().unwrap(),
                dns: vec!["192.0.2.53".parse().unwrap()],
            }
        );
    }

    #[test]
    fn parses_renew_and_stop() {
        assert_eq!(
            parse_control_event(r#"{"type":"RENEW","address":"198.51.100.2"}"#),
            Some(DhcpEvent::Renew {
                ip: "198.51.100.2".parse().unwrap()
            })
        );
        assert_eq!(
            parse_control_event(r#"{"command":"terminated"}"#),
            Some(DhcpEvent::Stopped)
        );
    }

    #[test]
    fn rejects_unknown_events() {
        assert!(parse_control_event(r#"{"event":"SOMETHING"}"#).is_none());
        assert!(parse_control_event("not json").is_none());
    }
}
