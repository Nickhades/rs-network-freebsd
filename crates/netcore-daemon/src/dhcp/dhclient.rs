//! dhclient backend.
//!
//! dhclient is still the default on stock FreeBSD images.  We keep it in the
//! foreground (-d) so the daemon owns the process and can stop it on
//! disconnect.  An existing rc.d dhclient for the interface is adopted rather
//! than duplicated.  Because dhclient has no event socket, lease changes are
//! observed through the interface address and the lease file; that polling is
//! contained inside this backend and is not exposed to the manager.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};

use super::lease::{LeaseDetails, lease_for_interface};
use super::{
    DhcpClient, DhcpEvent, Error, EventStream, Shared, shared_emit, shared_mark_external,
    shared_reset, shared_subscribe,
};
use crate::system::validate_ifname;

struct Runtime {
    stop: Arc<AtomicBool>,
    child: Option<Arc<Mutex<Child>>>,
    monitor: JoinHandle<()>,
    external: bool,
}

/// One dhclient process per interface.
pub(crate) struct DhclientBackend {
    dhclient: PathBuf,
    shared: Arc<Mutex<Shared>>,
    runtimes: Mutex<HashMap<String, Runtime>>,
}

impl DhclientBackend {
    pub(crate) fn new(dhclient: PathBuf) -> Self {
        Self {
            dhclient,
            shared: Arc::new(Mutex::new(Shared::new())),
            runtimes: Mutex::new(HashMap::new()),
        }
    }

    fn lease_paths(iface: &str) -> Vec<PathBuf> {
        vec![Path::new("/var/db").join(format!("dhclient.leases.{iface}"))]
    }
}

impl DhcpClient for DhclientBackend {
    fn start(&self, iface: &str) -> Result<(), Error> {
        validate_ifname(iface)?;
        let mut runtimes = self.runtimes.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(runtime) = runtimes.get(iface) {
            if runtime.external {
                return Ok(());
            }
            if let Some(child) = &runtime.child {
                let mut child = child.lock().unwrap_or_else(PoisonError::into_inner);
                if child.try_wait()?.is_none() {
                    return Ok(());
                }
            }
            // A dead runtime is replaced below; take ownership first.
            if let Some(dead) = runtimes.remove(iface) {
                dead.stop.store(true, Ordering::SeqCst);
                let _ = dead.monitor.join();
            }
        }

        shared_reset(&self.shared, iface);
        let external = running_external_dhclient(iface);
        let child = if external.is_some() {
            shared_mark_external(&self.shared, iface, true);
            None
        } else {
            shared_mark_external(&self.shared, iface, false);
            Some(self.spawn(iface)?)
        };
        let stop = Arc::new(AtomicBool::new(false));
        let monitor = spawn_monitor(
            iface.to_owned(),
            child.clone(),
            Arc::clone(&stop),
            Arc::clone(&self.shared),
        );
        runtimes.insert(
            iface.to_owned(),
            Runtime {
                stop,
                child,
                monitor,
                external: external.is_some(),
            },
        );
        if let Some(pid) = external {
            tracing::info!(ifname = iface, pid, "检测到已运行的 dhclient，沿用其租约");
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
        if !runtime.external
            && let Some(child) = &runtime.child
        {
            terminate(child);
        }
        shared_mark_external(&self.shared, iface, false);
        shared_emit(&self.shared, iface, DhcpEvent::Stopped);
        let _ = runtime.monitor.join();
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

impl DhclientBackend {
    fn spawn(&self, iface: &str) -> Result<Arc<Mutex<Child>>> {
        let mut command = Command::new(&self.dhclient);
        command
            .arg("-d")
            .arg("-q")
            .arg(iface)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("启动 {} -d -q {iface} 失败", self.dhclient.display()))?;
        if let Some(stdout) = child.stdout.take() {
            pipe_to_log(stdout, "dhclient stdout");
        }
        if let Some(stderr) = child.stderr.take() {
            pipe_to_log(stderr, "dhclient stderr");
        }
        tracing::info!(ifname = iface, pid = child.id(), "已启动 dhclient");
        Ok(Arc::new(Mutex::new(child)))
    }
}

fn pipe_to_log<R: std::io::Read + Send + 'static>(reader: R, label: &'static str) {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            tracing::debug!(target: "netcore_daemon::dhclient", "{label}: {line}");
        }
    });
}

fn spawn_monitor(
    iface: String,
    child: Option<Arc<Mutex<Child>>>,
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut bound = false;
        let mut last: Option<LeaseDetails> = None;
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            if let Some(child) = &child {
                let status = child
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .try_wait();
                match status {
                    Ok(Some(status)) => {
                        tracing::warn!(ifname = iface, %status, "dhclient 已退出");
                        shared_emit(&shared, &iface, DhcpEvent::Stopped);
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => tracing::warn!(ifname = iface, %error, "检查 dhclient 状态失败"),
                }
            }
            let lease = lease_for_interface(&iface, &DhclientBackend::lease_paths(&iface));
            match lease {
                Some(lease) => {
                    if !bound {
                        shared_emit(
                            &shared,
                            &iface,
                            DhcpEvent::Bound {
                                ip: lease.ip,
                                subnet: lease.subnet,
                                gateway: lease.gateway,
                                dns: lease.dns.clone(),
                            },
                        );
                        bound = true;
                    } else if last.as_ref() != Some(&lease) {
                        shared_emit(&shared, &iface, DhcpEvent::Renew { ip: lease.ip });
                    }
                    last = Some(lease);
                }
                None if bound => {
                    shared_emit(&shared, &iface, DhcpEvent::Expired);
                    bound = false;
                    last = None;
                }
                None => {}
            }
            thread::sleep(Duration::from_millis(300));
        }
    })
}

fn terminate(child: &Arc<Mutex<Child>>) {
    let mut child = child.lock().unwrap_or_else(PoisonError::into_inner);
    let pid = child.id();
    // Negative pid signals the process group created at spawn.
    // SAFETY: kill(2) with a pid we created and a valid signal.
    let rc = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGTERM) };
    if rc != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            tracing::warn!(pid, %error, "SIGTERM dhclient 失败");
        }
    }
    for _ in 0..50 {
        match child.try_wait() {
            Ok(Some(status)) => {
                tracing::debug!(pid, %status, "dhclient 已退出");
                return;
            }
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => {
                tracing::warn!(pid, %error, "等待 dhclient 失败");
                return;
            }
        }
    }
    tracing::warn!(pid, "dhclient 未在 5 秒内退出，发送 SIGKILL");
    let _ = child.kill();
    let _ = child.wait();
}

/// Check the standard FreeBSD pid file. A stale file does not count.
fn running_external_dhclient(ifname: &str) -> Option<u32> {
    let path = Path::new("/var/run/dhclient").join(format!("dhclient.{ifname}.pid"));
    let text = std::fs::read_to_string(path).ok()?;
    let pid: libc::pid_t = text.trim().parse().ok()?;
    if pid <= 0 {
        return None;
    }
    // SAFETY: signal 0 only probes existence and permission.
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 { Some(pid as u32) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn fake_dhclient_emits_bound() {
        let path =
            std::env::temp_dir().join(format!("netcore-fake-dhclient-{}", std::process::id()));
        std::fs::write(&path, "#!/bin/sh\nexec sleep 30\n").unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).unwrap();

        let backend = DhclientBackend::new(path.clone());
        let mut events = backend.subscribe("lo0");
        backend.start("lo0").expect("start fake dhclient");
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(events.next());
        });
        let event = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("bound event");
        assert!(matches!(event, Some(DhcpEvent::Bound { .. })));
        backend.stop("lo0").expect("stop fake dhclient");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn stale_lease_file_is_not_an_address() {
        // lease_for_interface relies on the kernel, so an unreadable/absent
        // lease file must not turn into a phantom address.
        assert!(lease_for_interface("netcore-no-such-iface", &[]).is_none());
    }
}
