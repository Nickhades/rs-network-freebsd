//! DHCP client abstraction.
//!
//! A backend owns one DHCP client process per interface and normalizes lease
//! notifications into DhcpEvent values consumed by the daemon state machine.

mod dhclient;
mod dhcpcd;
mod lease;
mod manager;

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, mpsc};

use anyhow::{Result, bail};

use crate::config::{Config, DhcpChoice};
use netcore_common::tools::{dhclient, dhcpcd, is_executable};

pub use manager::DhcpManager;

pub type Error = anyhow::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DhcpEvent {
    Bound {
        ip: IpAddr,
        subnet: u8,
        gateway: IpAddr,
        dns: Vec<IpAddr>,
    },
    Renew {
        ip: IpAddr,
    },
    Expired,
    Stopped,
}

pub trait EventStream: Iterator<Item = DhcpEvent> + Send {}

impl<T> EventStream for T where T: Iterator<Item = DhcpEvent> + Send {}

pub trait DhcpClient: Send + Sync {
    fn start(&self, iface: &str) -> Result<(), Error>;
    fn stop(&self, iface: &str) -> Result<(), Error>;
    fn subscribe(&self, iface: &str) -> Box<dyn EventStream>;
    fn was_external(&self, _iface: &str) -> bool {
        false
    }
    fn shutdown(&self) {}
}

pub(crate) struct ChannelStream {
    pub(crate) receiver: mpsc::Receiver<DhcpEvent>,
}

impl Iterator for ChannelStream {
    type Item = DhcpEvent;
    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.recv().ok()
    }
}

pub(crate) struct Shared {
    subscribers: HashMap<String, Vec<mpsc::Sender<DhcpEvent>>>,
    last: HashMap<String, DhcpEvent>,
    external: HashSet<String>,
}

impl Shared {
    pub(crate) fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
            last: HashMap::new(),
            external: HashSet::new(),
        }
    }
    pub(crate) fn subscribe(&mut self, iface: &str) -> Box<dyn EventStream> {
        let (sender, receiver) = mpsc::channel();
        if let Some(event) = self.last.get(iface) {
            let _ = sender.send(event.clone());
        }
        self.subscribers
            .entry(iface.to_owned())
            .or_default()
            .push(sender);
        Box::new(ChannelStream { receiver })
    }
    pub(crate) fn emit(&mut self, iface: &str, event: DhcpEvent) {
        self.last.insert(iface.to_owned(), event.clone());
        if let Some(subscribers) = self.subscribers.get_mut(iface) {
            subscribers.retain(|sender| sender.send(event.clone()).is_ok());
        }
    }
    pub(crate) fn reset(&mut self, iface: &str) {
        self.last.remove(iface);
    }

    pub(crate) fn mark_external(&mut self, iface: &str, external: bool) {
        if external {
            self.external.insert(iface.to_owned());
        } else {
            self.external.remove(iface);
        }
    }
    pub(crate) fn is_external(&self, iface: &str) -> bool {
        self.external.contains(iface)
    }
}

pub(crate) fn shared_subscribe(shared: &Arc<Mutex<Shared>>, iface: &str) -> Box<dyn EventStream> {
    shared
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .subscribe(iface)
}

pub(crate) fn shared_emit(shared: &Arc<Mutex<Shared>>, iface: &str, event: DhcpEvent) {
    shared
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .emit(iface, event);
}

pub(crate) fn shared_reset(shared: &Arc<Mutex<Shared>>, iface: &str) {
    shared
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .reset(iface);
}

pub(crate) fn shared_mark_external(shared: &Arc<Mutex<Shared>>, iface: &str, external: bool) {
    shared
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .mark_external(iface, external);
}

pub fn client_from_config(config: &Config) -> Result<Arc<dyn DhcpClient>> {
    let (choice, path) = match config.dhcp_backend {
        DhcpChoice::Dhcpcd => (
            DhcpChoice::Dhcpcd,
            config
                .dhcpcd
                .clone()
                .or_else(dhcpcd)
                .ok_or_else(|| anyhow::anyhow!("未找到 dhcpcd；请用 --dhcpcd 指定"))?,
        ),
        DhcpChoice::Dhclient => (
            DhcpChoice::Dhclient,
            config
                .dhclient
                .clone()
                .or_else(dhclient)
                .ok_or_else(|| anyhow::anyhow!("未找到 dhclient；请用 --dhclient 指定"))?,
        ),
        DhcpChoice::Auto => {
            if let Some(path) = config
                .dhcpcd
                .clone()
                .or_else(dhcpcd)
                .filter(|p| is_executable(p))
            {
                (DhcpChoice::Dhcpcd, path)
            } else if let Some(path) = config
                .dhclient
                .clone()
                .or_else(dhclient)
                .filter(|p| is_executable(p))
            {
                (DhcpChoice::Dhclient, path)
            } else {
                bail!("未找到可用的 DHCP 客户端（dhcpcd 或 dhclient）");
            }
        }
    };
    if !is_executable(&path) {
        bail!("DHCP 客户端不可执行：{}", path.display());
    }
    match choice {
        DhcpChoice::Dhcpcd => Ok(Arc::new(dhcpcd::DhcpcdBackend::new(path))),
        DhcpChoice::Dhclient => Ok(Arc::new(dhclient::DhclientBackend::new(path))),
        DhcpChoice::Auto => unreachable!("auto is resolved above"),
    }
}

#[must_use]
pub fn selected_backend(config: &Config) -> &'static str {
    match config.dhcp_backend {
        DhcpChoice::Dhcpcd => "dhcpcd",
        DhcpChoice::Dhclient => "dhclient",
        DhcpChoice::Auto => "auto",
    }
}

pub(crate) fn dhcpcd_control_paths(iface: &str) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/var/run/dhcpcd").join(format!("{iface}.sock")),
        PathBuf::from(format!("/var/run/dhcpcd-{iface}.sock")),
    ]
}

pub(crate) fn dhcpcd_lease_paths(iface: &str) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/var/db/dhcpcd").join(format!("{iface}.lease")),
        PathBuf::from("/var/db/dhcpcd").join(format!("interface-{iface}.lease")),
    ]
}
