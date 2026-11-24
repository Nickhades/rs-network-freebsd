//! Asynchronous DHCP orchestration on top of the synchronous backend trait.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result};

use super::{DhcpClient, DhcpEvent, client_from_config};
use crate::config::Config;
use crate::system::{has_ipv4, interfaces, validate_ifname};

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
            Self::Started => "DHCP 客户端已启动",
            Self::AlreadyConfigured => "接口已有 IPv4 地址",
            Self::AlreadyRunningExternally => "检测到外部 DHCP 客户端，沿用其租约",
        }
    }
}

/// Owns the selected backend and serializes ensure/stop requests.
pub struct DhcpManager {
    client: Arc<dyn DhcpClient>,
    timeout: Duration,
    enabled: bool,
    started: Mutex<HashSet<String>>,
}

impl DhcpManager {
    #[must_use]
    pub fn new(client: Arc<dyn DhcpClient>, timeout: Duration, enabled: bool) -> Self {
        Self {
            client,
            timeout,
            enabled,
            started: Mutex::new(HashSet::new()),
        }
    }

    pub fn from_config(config: &Config) -> Result<Self> {
        let client = client_from_config(config)?;
        Ok(Self::new(
            client,
            Duration::from_secs(config.dhcp_timeout.max(1)),
            !config.no_dhcp,
        ))
    }

    /// Ensure the interface has a lease.  The backend's event stream is the
    /// authority for success; the old interface-address polling remains only
    /// inside the dhcpcd backend.
    pub async fn ensure(&self, ifname: &str, force: bool) -> Result<DhcpOutcome> {
        validate_ifname(ifname)?;
        if !self.enabled {
            return Ok(DhcpOutcome::AlreadyConfigured);
        }
        if !force && interface_has_ipv4(ifname)? {
            return Ok(DhcpOutcome::AlreadyConfigured);
        }

        // Start first, then subscribe.  The backend retains the last event for
        // replay, so an immediate BOUND is not lost, and a stale Stopped event
        // from a previous run cannot be mistaken for this run's failure.
        self.client
            .start(ifname)
            .with_context(|| format!("启动 DHCP 客户端失败 ({ifname})"))?;
        let mut stream = self.client.subscribe(ifname);
        self.started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(ifname.to_owned());

        let (send, receive) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let result = match stream.next() {
                Some(DhcpEvent::Bound { .. } | DhcpEvent::Renew { .. }) => Ok(()),
                Some(DhcpEvent::Expired) => Err(anyhow::anyhow!("DHCP 租约已过期")),
                Some(DhcpEvent::Stopped) => Err(anyhow::anyhow!("DHCP 客户端在获得租约前退出")),
                None => Err(anyhow::anyhow!("DHCP 事件流已关闭")),
            };
            let _ = send.send(result);
        });

        let result = match tokio::time::timeout(self.timeout, receive).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(anyhow::anyhow!("DHCP 事件等待任务已取消")),
            Err(_) => Err(anyhow::anyhow!("等待 {ifname} 获取 DHCP 租约超时")),
        };
        if result.is_err() {
            let _ = self.client.stop(ifname);
            self.started
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(ifname);
            return result.map(|_| DhcpOutcome::Started);
        }
        Ok(if self.client.was_external(ifname) {
            DhcpOutcome::AlreadyRunningExternally
        } else {
            DhcpOutcome::Started
        })
    }

    /// Stop our client.  Externally managed clients are left running.
    pub async fn stop(&self, ifname: &str) {
        let _ = self.client.stop(ifname);
        self.started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(ifname);
    }

    /// Kept for the manager refresh loop.  Process death is now reported by
    /// the backend event stream, so there is no child table to reap here.
    pub fn reap_all(&self) {}

    pub async fn shutdown(&self) {
        let names: Vec<String> = self
            .started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect();
        for name in names {
            self.stop(&name).await;
        }
        self.client.shutdown();
    }
}

fn interface_has_ipv4(ifname: &str) -> Result<bool> {
    Ok(interfaces()?
        .iter()
        .any(|info| info.ifname == ifname && has_ipv4(info)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dhcp::{EventStream, Shared, shared_emit, shared_subscribe};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct MockClient {
        events: Mutex<Option<Vec<DhcpEvent>>>,
        started: AtomicBool,
        shared: Arc<Mutex<Shared>>,
    }

    impl MockClient {
        fn new(events: Vec<DhcpEvent>) -> Self {
            Self {
                events: Mutex::new(Some(events)),
                started: AtomicBool::new(false),
                shared: Arc::new(Mutex::new(Shared::new())),
            }
        }
    }

    impl DhcpClient for MockClient {
        fn start(&self, iface: &str) -> std::result::Result<(), super::super::Error> {
            self.started.store(true, Ordering::SeqCst);
            // Emit through the shared stream, matching real backends.
            if let Some(events) = self.events.lock().unwrap().take() {
                for event in events {
                    shared_emit(&self.shared, iface, event);
                }
            }
            Ok(())
        }

        fn stop(&self, _iface: &str) -> std::result::Result<(), super::super::Error> {
            Ok(())
        }

        fn subscribe(&self, iface: &str) -> Box<dyn EventStream> {
            shared_subscribe(&self.shared, iface)
        }
    }

    #[tokio::test]
    async fn ensure_waits_for_bound_event() {
        let client = Arc::new(MockClient::new(vec![DhcpEvent::Bound {
            ip: "192.0.2.10".parse().unwrap(),
            subnet: 24,
            gateway: "192.0.2.1".parse().unwrap(),
            dns: vec![],
        }]));
        let manager = DhcpManager::new(client.clone(), Duration::from_secs(2), true);
        let outcome = manager.ensure("wlan0", true).await.expect("ensure");
        assert_eq!(outcome, DhcpOutcome::Started);
        assert!(client.started.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn disabled_manager_does_not_start() {
        let client = Arc::new(MockClient::new(Vec::new()));
        let manager = DhcpManager::new(client.clone(), Duration::from_secs(1), false);
        assert_eq!(
            manager.ensure("wlan0", true).await.expect("ensure"),
            DhcpOutcome::AlreadyConfigured
        );
        assert!(!client.started.load(Ordering::SeqCst));
    }
}
