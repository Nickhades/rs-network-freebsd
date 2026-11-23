//! dhclient lifecycle management.
//!
//! FreeBSD's dhclient configures the interface and then detaches or keeps
//! renewing. We always run it with -d (foreground) so the daemon owns the
//! process, can capture diagnostics and can stop it on disconnect. If rc.d or
//! the administrator already runs a dhclient for the interface we detect its
//! pid file and adopt the externally managed lease instead of starting a
//! duplicate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::system::{has_ipv4, interfaces, validate_ifname};

const STDERR_TAIL_LIMIT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DhcpOutcome {
    Started,
    AlreadyConfigured,
    AlreadyRunningExternally,
}

impl DhcpOutcome {
    #[must_use]
    pub const fn message(&self) -> &'static str {
        match self {
            Self::Started => "dhclient 已启动",
            Self::AlreadyConfigured => "接口已有 IPv4 地址",
            Self::AlreadyRunningExternally => "检测到外部 dhclient，沿用其租约",
        }
    }
}

enum LeaseAction {
    Wait,
    External(u32),
    Spawn,
}

struct Handle {
    child: Child,
    stderr_tail: Arc<Mutex<String>>,
}

#[derive(Default)]
struct Inner {
    handles: HashMap<String, Handle>,
    external: HashMap<String, u32>,
}

/// Owns one dhclient process per interface.
#[derive(Clone)]
pub struct DhcpManager {
    inner: Arc<Mutex<Inner>>,
    dhclient: PathBuf,
    timeout: Duration,
    enabled: bool,
}

impl DhcpManager {
    #[must_use]
    pub fn new(dhclient: PathBuf, timeout: Duration, enabled: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            dhclient,
            timeout,
            enabled,
        }
    }

    /// Ensure the interface obtains a lease, either by running our dhclient or
    /// by waiting for one that is already running.
    pub async fn ensure(&self, ifname: &str, force: bool) -> Result<DhcpOutcome> {
        validate_ifname(ifname)?;
        if !self.enabled {
            return Ok(DhcpOutcome::AlreadyConfigured);
        }

        if !force && interface_has_ipv4(ifname)? {
            return Ok(DhcpOutcome::AlreadyConfigured);
        }

        let action = {
            let inner = self.inner.lock().expect("dhcp mutex poisoned");
            if inner.handles.contains_key(ifname) {
                LeaseAction::Wait
            } else if let Some(pid) = running_external_dhclient(ifname) {
                LeaseAction::External(pid)
            } else {
                LeaseAction::Spawn
            }
        };
        match action {
            LeaseAction::Wait => {}
            LeaseAction::External(pid) => {
                tracing::info!(ifname, pid, "检测到已运行的 dhclient，沿用其租约");
                self.inner
                    .lock()
                    .expect("dhcp mutex poisoned")
                    .external
                    .insert(ifname.to_owned(), pid);
                self.wait_for_lease(ifname, None).await?;
                return Ok(DhcpOutcome::AlreadyRunningExternally);
            }
            LeaseAction::Spawn => self.spawn(ifname)?,
        }

        self.wait_for_lease(ifname, None).await?;
        Ok(DhcpOutcome::Started)
    }

    fn spawn(&self, ifname: &str) -> Result<()> {
        let mut child = Command::new(&self.dhclient)
            .arg("-d")
            .arg("-q")
            .arg(ifname)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0)
            .spawn()
            .with_context(|| format!("启动 {} -d -q {ifname} 失败", self.dhclient.display()))?;
        let pid = child.id().context("dhclient 没有 pid")?;
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "netcore_daemon::dhclient", "{line}");
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let tail = Arc::clone(&stderr_tail);
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "netcore_daemon::dhclient", "{line}");
                    if let Ok(mut tail) = tail.lock() {
                        tail.push_str(&line);
                        tail.push('\n');
                        if tail.len() > STDERR_TAIL_LIMIT {
                            let drain = tail.len() - STDERR_TAIL_LIMIT;
                            tail.drain(..drain);
                        }
                    }
                }
            });
        }
        self.inner
            .lock()
            .expect("dhcp mutex poisoned")
            .handles
            .insert(ifname.to_owned(), Handle { child, stderr_tail });
        tracing::info!(ifname, pid, "已启动 dhclient");
        Ok(())
    }

    async fn wait_for_lease(&self, ifname: &str, _reserved: Option<()>) -> Result<()> {
        let deadline = tokio::time::Instant::now() + self.timeout;
        loop {
            if interface_has_ipv4(ifname)? {
                return Ok(());
            }
            if let Some((status, tail)) = self.reap(ifname) {
                bail!(
                    "dhclient 在获取地址前退出 ({status}){}",
                    if tail.trim().is_empty() {
                        String::new()
                    } else {
                        format!(": {}", tail.trim())
                    }
                );
            }
            if tokio::time::Instant::now() >= deadline {
                self.stop(ifname).await;
                bail!("等待 {ifname} 获取 DHCP 租约超时");
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }

    fn reap(&self, ifname: &str) -> Option<(String, String)> {
        let mut inner = self.inner.lock().expect("dhcp mutex poisoned");
        let handle = inner.handles.get_mut(ifname)?;
        match handle.child.try_wait() {
            Ok(Some(status)) => {
                let tail = handle
                    .stderr_tail
                    .lock()
                    .map_or_else(|_| String::new(), |tail| tail.clone());
                inner.handles.remove(ifname);
                Some((status.to_string(), tail))
            }
            Ok(None) => None,
            Err(error) => {
                tracing::warn!(ifname, %error, "检查 dhclient 状态失败");
                None
            }
        }
    }

    /// Called by the manager's periodic refresh to remove dead children.
    pub fn reap_all(&self) {
        let names: Vec<String> = self
            .inner
            .lock()
            .expect("dhcp mutex poisoned")
            .handles
            .keys()
            .cloned()
            .collect();
        for name in names {
            if let Some((status, tail)) = self.reap(&name) {
                tracing::warn!(ifname = name, %status, tail, "dhclient 已退出");
            }
        }
    }

    /// Stop a process started by us. External leases are left untouched.
    pub async fn stop(&self, ifname: &str) {
        let handle = {
            let mut inner = self.inner.lock().expect("dhcp mutex poisoned");
            inner.external.remove(ifname);
            inner.handles.remove(ifname)
        };
        if let Some(mut handle) = handle {
            let Some(pid) = handle.child.id() else {
                return;
            };
            tracing::info!(ifname, pid, "停止 dhclient");
            // Negative pid signals the whole process group created at spawn.
            // SAFETY: kill(2) with a pid we created and a valid signal.
            let rc = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGTERM) };
            if rc != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    tracing::warn!(ifname, %error, "SIGTERM dhclient 失败");
                }
            }
            match tokio::time::timeout(Duration::from_secs(5), handle.child.wait()).await {
                Ok(Ok(status)) => tracing::debug!(ifname, %status, "dhclient 已退出"),
                Ok(Err(error)) => tracing::warn!(ifname, %error, "等待 dhclient 失败"),
                Err(_) => {
                    tracing::warn!(ifname, "dhclient 未在 5 秒内退出，发送 SIGKILL");
                    let _ = handle.child.start_kill();
                    let _ = handle.child.wait().await;
                }
            }
        }
    }

    pub async fn shutdown(&self) {
        let names: Vec<String> = self
            .inner
            .lock()
            .expect("dhcp mutex poisoned")
            .handles
            .keys()
            .cloned()
            .collect();
        for name in names {
            self.stop(&name).await;
        }
    }
}

fn interface_has_ipv4(ifname: &str) -> Result<bool> {
    Ok(interfaces()?
        .iter()
        .any(|info| info.ifname == ifname && has_ipv4(info)))
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

    #[test]
    fn outcome_messages_are_nonempty() {
        assert!(!DhcpOutcome::Started.message().is_empty());
        assert!(!DhcpOutcome::AlreadyConfigured.message().is_empty());
        assert!(!DhcpOutcome::AlreadyRunningExternally.message().is_empty());
    }

    #[tokio::test]
    async fn disabled_manager_reports_configured() {
        let manager = DhcpManager::new(
            PathBuf::from("/sbin/dhclient"),
            Duration::from_secs(1),
            false,
        );
        assert_eq!(
            manager.ensure("lo0", false).await.unwrap(),
            DhcpOutcome::AlreadyConfigured
        );
    }
}
