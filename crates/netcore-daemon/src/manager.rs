//! The network manager state machine and operation orchestration.
//!
//! Device state is derived from kernel facts (getifaddrs) plus wpa_supplicant
//! association state. Operations are serialized so two GUI clicks cannot race
//! dhclient or wpa_supplicant mutations. Every successful refresh emits one
//! deduplicated snapshot, which the D-Bus service fans out as a signal.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use netcore_common::ssid::same_ssid;
use netcore_common::{
    ActionResult, Connectivity, DeviceKind, DeviceState, ErrorCode, ManagerSnapshot, ManagerState,
    NetworkDevice, PROTOCOL_VERSION, ScanResult, Security, VERSION, WifiNetwork,
};
use tokio::sync::{Mutex, RwLock, broadcast};
use tokio::time::timeout;

use crate::config::Config;
use crate::dhcp::DhcpManager;
use crate::error::ManagerError;
use crate::iface::{self, InterfaceInfo, is_virtual};
use crate::link::{LinkEvent, LinkState};
use crate::system::{self, current_epoch, hostname, set_link_up, validate_ifname};
use crate::wifi::{WifiService, WifiStatus, deduplicate};

#[derive(Debug, Clone)]
pub enum ManagerEvent {
    Snapshot(ManagerSnapshot),
    ScanFinished {
        ifname: String,
        networks: Vec<WifiNetwork>,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone)]
struct OperationState {
    ifname: String,
    state: DeviceState,
}

struct Inner {
    ifconfig: std::path::PathBuf,
    dhcp: DhcpManager,
    no_dhcp: bool,
    wifi: WifiService,
    refresh_lock: Mutex<()>,
    op_lock: Mutex<()>,
    networks: RwLock<HashMap<String, Vec<WifiNetwork>>>,
    op_state: RwLock<Option<OperationState>>,
    link_states: RwLock<HashMap<String, LinkState>>,
    last_errors: RwLock<HashMap<String, String>>,
    active_device: RwLock<Option<String>>,
    wifi_enabled: AtomicBool,
    netlink_active: Arc<AtomicBool>,
    events: broadcast::Sender<ManagerEvent>,
    last_key: std::sync::Mutex<Option<String>>,
    hostname: String,
    wireless_ifnames: std::sync::RwLock<HashSet<String>>,
}

/// Cheaply cloneable manager handle.
#[derive(Clone)]
pub struct Manager {
    inner: Arc<Inner>,
}

impl Manager {
    pub async fn new(config: &Config, wifi: WifiService) -> Self {
        let (events, _) = broadcast::channel(128);
        let inner = Arc::new(Inner {
            ifconfig: config.ifconfig.clone().expect("ifconfig resolved"),
            dhcp: DhcpManager::from_config(config).expect("DHCP backend resolved"),
            no_dhcp: config.no_dhcp,
            wifi,
            refresh_lock: Mutex::new(()),
            op_lock: Mutex::new(()),
            networks: RwLock::new(HashMap::new()),
            op_state: RwLock::new(None),
            link_states: RwLock::new(HashMap::new()),
            last_errors: RwLock::new(HashMap::new()),
            active_device: RwLock::new(None),
            wifi_enabled: AtomicBool::new(true),
            netlink_active: Arc::new(AtomicBool::new(false)),
            events,
            last_key: std::sync::Mutex::new(None),
            hostname: hostname(),
            wireless_ifnames: std::sync::RwLock::new(HashSet::new()),
        });
        let manager = Self { inner };
        manager.refresh().await;
        manager
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ManagerEvent> {
        self.inner.events.subscribe()
    }

    #[must_use]
    pub fn netlink_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.inner.netlink_active)
    }

    /// Recompute the snapshot, emit it if it changed, and return it.
    pub async fn refresh(&self) -> ManagerSnapshot {
        let _refresh = self.inner.refresh_lock.lock().await;
        self.inner.dhcp.reap_all();
        // state_lock is only used to make the critical-point intent explicit;
        // network reads are performed without holding it across awaits.
        let interfaces = match system::interfaces() {
            Ok(interfaces) => interfaces,
            Err(error) => {
                tracing::error!(%error, "枚举网络接口失败");
                Vec::new()
            }
        };
        let wireless = match iface::wireless_interfaces(&self.inner.ifconfig).await {
            Ok(names) => names,
            Err(error) => {
                tracing::warn!(%error, "读取无线接口分组失败；沿用上一次结果");
                self.inner
                    .wireless_ifnames
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
            }
        };
        *self
            .inner
            .wireless_ifnames
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = wireless.clone();
        let networks = self.inner.networks.read().await.clone();
        let op_state = self.inner.op_state.read().await.clone();
        let errors = self.inner.last_errors.read().await.clone();
        let active_device = self.inner.active_device.read().await.clone();
        let wifi_enabled = self.inner.wifi_enabled.load(Ordering::SeqCst);

        let present: HashSet<String> = interfaces.iter().map(|info| info.ifname.clone()).collect();
        let vanished: Vec<String> = self
            .inner
            .link_states
            .read()
            .await
            .keys()
            .filter(|name| !present.contains(*name))
            .cloned()
            .collect();
        for name in vanished {
            self.advance_link(&name, LinkEvent::LinkLost).await;
        }
        let mut devices = Vec::new();
        for mut info in interfaces {
            info.wireless = wireless.contains(&info.ifname);
            if info.is_loopback() || (is_virtual(&info.ifname) && !info.wireless) {
                continue;
            }
            let kind = info.kind();
            let mut device = iface::to_device(&info, DeviceState::Disconnected);
            device.managed = true;
            let mut wifi_status: Option<WifiStatus> = None;

            if kind == DeviceKind::Wifi {
                if !wifi_enabled {
                    device.state = DeviceState::Unavailable;
                } else if self.inner.wifi.available() {
                    match timeout(Duration::from_secs(3), self.inner.wifi.status(&info.ifname))
                        .await
                    {
                        Ok(Ok(status)) => {
                            wifi_status = Some(status);
                        }
                        Ok(Err(error)) => {
                            tracing::debug!(ifname = %info.ifname, %error, "读取 Wi-Fi 状态失败");
                        }
                        Err(_) => {
                            tracing::debug!(ifname = %info.ifname, "读取 Wi-Fi 状态超时");
                        }
                    }
                } else {
                    device.state = DeviceState::Unavailable;
                    device.last_error = Some("wpa_supplicant 后端不可用".to_owned());
                }
                if let Some(status) = &wifi_status {
                    device.ssid = status.ssid.clone();
                    device.ssid_hex = status.ssid_hex.clone();
                    if status.connected {
                        device.state = DeviceState::Connected;
                    }
                }
                device.networks = networks.get(&info.ifname).cloned().unwrap_or_default();
                if let Some(status) = &wifi_status {
                    for network in &mut device.networks {
                        network.connected = status.connected
                            && same_ssid(
                                status.ssid.as_deref().unwrap_or_default(),
                                status.ssid_hex.as_deref(),
                                &network.ssid,
                                network.ssid_hex.as_deref(),
                            );
                    }
                }
                device.networks = deduplicate(device.networks);
            }

            if kind != DeviceKind::Wifi || wifi_status.is_none() {
                if !info.enabled() {
                    device.state = DeviceState::Disconnected;
                } else if !info.ipv4.is_empty() {
                    device.state = DeviceState::Connected;
                } else {
                    device.state = DeviceState::Disconnected;
                }
            }
            if !info.enabled() && kind != DeviceKind::Wifi {
                device.state = DeviceState::Disconnected;
            }
            if kind == DeviceKind::Wifi && !wifi_enabled {
                device.state = DeviceState::Unavailable;
                device.ssid = None;
                device.ssid_hex = None;
            } else if kind == DeviceKind::Wifi && !info.enabled() {
                device.state = DeviceState::Disconnected;
                device.ssid = None;
                device.ssid_hex = None;
            }

            if let Some(error) = errors.get(&info.ifname) {
                device.state = DeviceState::Failed;
                device.last_error = Some(error.clone());
            }
            if let Some(op) = &op_state
                && op.ifname == info.ifname
            {
                device.state = op.state;
                device.last_error = None;
            }
            devices.push(device);
        }

        devices.sort_by(|left, right| {
            let rank = |kind: DeviceKind| match kind {
                DeviceKind::Wifi => 0,
                DeviceKind::Ethernet => 1,
                DeviceKind::Other => 2,
                DeviceKind::Loopback => 3,
            };
            rank(left.kind)
                .cmp(&rank(right.kind))
                .then_with(|| left.ifname.cmp(&right.ifname))
        });

        let wifi_available = self.inner.wifi.available()
            && devices.iter().any(|device| device.kind == DeviceKind::Wifi);
        let any_connected = devices
            .iter()
            .any(|device| device.state == DeviceState::Connected);
        let any_ip = devices
            .iter()
            .any(|device| device.state == DeviceState::Connected && !device.ipv4.is_empty());
        let state = if op_state
            .as_ref()
            .is_some_and(|op| matches!(op.state, DeviceState::Connecting | DeviceState::IpConfig))
        {
            ManagerState::Connecting
        } else if op_state
            .as_ref()
            .is_some_and(|op| op.state == DeviceState::Disconnecting)
        {
            ManagerState::Disconnecting
        } else if any_connected {
            ManagerState::Connected
        } else {
            ManagerState::Offline
        };
        let connectivity = if any_ip {
            Connectivity::Local
        } else if any_connected {
            Connectivity::Unknown
        } else {
            Connectivity::None
        };

        let mut snapshot = ManagerSnapshot {
            protocol_version: PROTOCOL_VERSION,
            version: VERSION.to_owned(),
            hostname: self.inner.hostname.clone(),
            state,
            connectivity,
            wifi_enabled,
            wifi_available,
            wpa_backend: self.inner.wifi.kind().to_owned(),
            netlink_active: self.inner.netlink_active.load(Ordering::SeqCst),
            updated_at: current_epoch(),
            devices,
            active_device,
        };

        let key = {
            let mut key_snapshot = snapshot.clone();
            key_snapshot.updated_at = 0;
            key_snapshot.to_json()
        };
        let changed = {
            let mut last = self
                .inner
                .last_key
                .lock()
                .expect("snapshot key mutex poisoned");
            if last.as_deref() == Some(key.as_str()) {
                false
            } else {
                *last = Some(key);
                true
            }
        };
        if changed {
            snapshot.updated_at = current_epoch();
            let _ = self
                .inner
                .events
                .send(ManagerEvent::Snapshot(snapshot.clone()));
        }
        snapshot
    }

    pub async fn scan_wifi(&self, ifname: &str) -> Result<ScanResult, ManagerError> {
        let _guard = self
            .inner
            .op_lock
            .try_lock()
            .map_err(|_| ManagerError::busy("另一个网络操作正在进行"))?;
        validate_ifname(ifname).map_err(|error| ManagerError::invalid(error.to_string()))?;
        self.require_wifi_interface(ifname)?;
        if !self.inner.wifi.available() {
            return Err(ManagerError::unavailable("wpa_supplicant 后端不可用"));
        }
        if !self.inner.wifi_enabled.load(Ordering::SeqCst) {
            return Err(ManagerError::unavailable("Wi-Fi 已被禁用"));
        }
        let previous_link = self
            .inner
            .link_states
            .read()
            .await
            .get(ifname)
            .copied()
            .unwrap_or(LinkState::Down);
        self.advance_link(ifname, LinkEvent::ScanRequested).await;
        let status = self.inner.wifi.status(ifname).await.ok();
        let mut networks = self
            .inner
            .wifi
            .scan(ifname)
            .await
            .map_err(ManagerError::from)?;
        if let Some(status) = status {
            for network in &mut networks {
                network.connected = status.connected
                    && same_ssid(
                        status.ssid.as_deref().unwrap_or_default(),
                        status.ssid_hex.as_deref(),
                        &network.ssid,
                        network.ssid_hex.as_deref(),
                    );
            }
        }
        let networks = deduplicate(networks);
        self.inner
            .networks
            .write()
            .await
            .insert(ifname.to_owned(), networks.clone());
        let _ = self.inner.events.send(ManagerEvent::ScanFinished {
            ifname: ifname.to_owned(),
            networks: networks.clone(),
        });
        self.advance_link(ifname, LinkEvent::ScanComplete).await;
        if previous_link == LinkState::Connected {
            self.set_link_state(ifname, LinkState::Connected).await;
        }
        self.refresh().await;
        Ok(ScanResult {
            ifname: ifname.to_owned(),
            networks,
            scanned_at: current_epoch(),
        })
    }

    pub async fn connect_wifi(
        &self,
        ifname: &str,
        ssid: &str,
        password: &str,
        hidden: bool,
    ) -> Result<NetworkDevice, ManagerError> {
        let _guard = self
            .inner
            .op_lock
            .try_lock()
            .map_err(|_| ManagerError::busy("另一个网络操作正在进行"))?;
        self.require_privilege()?;
        validate_ifname(ifname).map_err(|error| ManagerError::invalid(error.to_string()))?;
        self.require_wifi_interface(ifname)?;
        if !self.inner.wifi.available() {
            return Err(ManagerError::unavailable("wpa_supplicant 后端不可用"));
        }
        if !self.inner.wifi_enabled.load(Ordering::SeqCst) {
            return Err(ManagerError::unavailable("请先启用 Wi-Fi"));
        }
        if ssid.is_empty() {
            return Err(ManagerError::invalid("SSID 不能为空"));
        }
        if ssid.len() > 32 {
            return Err(ManagerError::invalid("SSID 不能超过 32 字节"));
        }
        let security = self.resolve_security(ifname, ssid, password, hidden).await;
        if matches!(security, Security::Enterprise) {
            return Err(ManagerError::unsupported(
                "暂不支持企业级 802.1X 网络，请使用 wpa_supplicant 配置文件",
            ));
        }
        if matches!(security, Security::Wep) {
            return Err(ManagerError::unsupported(
                "出于安全原因，netcore 不支持 WEP",
            ));
        }
        if security.requires_password() && password.is_empty() {
            return Err(ManagerError::invalid("该网络需要密码"));
        }

        self.inner.dhcp.stop(ifname).await;
        self.clear_error(ifname).await;
        self.set_link_state(ifname, LinkState::Down).await;
        self.advance_link(ifname, LinkEvent::StartAuthentication)
            .await;
        self.set_operation(ifname, DeviceState::Connecting).await;
        self.refresh().await;

        let result = self
            .connect_wifi_inner(ifname, ssid, password, security, hidden)
            .await;
        match result {
            Ok(()) => {
                self.inner
                    .active_device
                    .write()
                    .await
                    .replace(ifname.to_owned());
                self.clear_operation().await;
                self.clear_error(ifname).await;
                let snapshot = self.refresh().await;
                snapshot
                    .device(ifname)
                    .cloned()
                    .ok_or_else(|| ManagerError::operation("连接后接口从系统中消失"))
            }
            Err(error) => {
                self.advance_link(ifname, LinkEvent::Failure).await;
                self.set_link_state(ifname, LinkState::Down).await;
                self.set_operation(ifname, DeviceState::Connecting).await;
                self.inner.dhcp.stop(ifname).await;
                let _ = self.inner.wifi.disconnect(ifname).await;
                self.set_error(ifname, &error.message).await;
                self.clear_operation().await;
                self.refresh().await;
                let _ = self.inner.events.send(ManagerEvent::Error {
                    code: error.code,
                    message: error.message.clone(),
                });
                Err(error)
            }
        }
    }

    async fn connect_wifi_inner(
        &self,
        ifname: &str,
        ssid: &str,
        password: &str,
        security: Security,
        hidden: bool,
    ) -> Result<(), ManagerError> {
        set_link_up(&self.inner.ifconfig, ifname, true)
            .await
            .map_err(|error| ManagerError::operation(format!("启用 {ifname} 失败: {error:#}")))?;
        self.inner
            .wifi
            .connect(ifname, ssid, password, security, hidden)
            .await
            .map_err(|error| ManagerError::operation(format!("Wi-Fi 关联失败: {error:#}")))?;

        self.advance_link(ifname, LinkEvent::Associated).await;
        self.set_operation(ifname, DeviceState::IpConfig).await;
        self.refresh().await;

        if !self.inner.no_dhcp {
            self.advance_link(ifname, LinkEvent::DhcpStart).await;
            let outcome = match self.inner.dhcp.ensure(ifname, true).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.advance_link(ifname, LinkEvent::DhcpExpired).await;
                    return Err(ManagerError::operation(format!("DHCP 配置失败: {error:#}")));
                }
            };
            self.advance_link(ifname, LinkEvent::DhcpBound).await;
            tracing::info!(ifname, message = outcome.message(), "DHCP 状态");
        } else {
            self.set_link_state(ifname, LinkState::Connected).await;
        }
        Ok(())
    }

    pub async fn disconnect_device(&self, ifname: &str) -> Result<ActionResult, ManagerError> {
        let _guard = self
            .inner
            .op_lock
            .try_lock()
            .map_err(|_| ManagerError::busy("另一个网络操作正在进行"))?;
        self.require_privilege()?;
        validate_ifname(ifname).map_err(|error| ManagerError::invalid(error.to_string()))?;
        let info = self.lookup_interface(ifname)?;
        let device = self.disconnect_locked(&info).await?;
        Ok(ActionResult {
            message: format!("已断开 {ifname}"),
            device: Some(device),
        })
    }

    async fn disconnect_locked(&self, info: &InterfaceInfo) -> Result<NetworkDevice, ManagerError> {
        let ifname = &info.ifname;
        self.advance_link(ifname, LinkEvent::DisconnectRequested)
            .await;
        self.set_operation(ifname, DeviceState::Disconnecting).await;
        self.refresh().await;
        let mut first_error: Option<ManagerError> = None;
        self.inner.dhcp.stop(ifname).await;
        if info.kind() == DeviceKind::Wifi
            && let Err(error) = self.inner.wifi.disconnect(ifname).await
        {
            first_error = Some(ManagerError::operation(format!(
                "断开 Wi-Fi 失败: {error:#}"
            )));
        }
        if let Err(error) = set_link_up(&self.inner.ifconfig, ifname, false).await {
            first_error.get_or_insert_with(|| {
                ManagerError::operation(format!("关闭 {ifname} 失败: {error:#}"))
            });
        }
        self.clear_operation().await;
        self.advance_link(ifname, LinkEvent::Disconnected).await;
        self.set_link_state(ifname, LinkState::Down).await;
        let mut active = self.inner.active_device.write().await;
        if active.as_deref() == Some(ifname.as_str()) {
            *active = None;
        }
        drop(active);
        if let Some(error) = &first_error {
            self.set_error(ifname, &error.message).await;
        } else {
            self.clear_error(ifname).await;
        }
        let snapshot = self.refresh().await;
        if let Some(error) = first_error {
            return Err(error);
        }
        snapshot
            .device(ifname)
            .cloned()
            .ok_or_else(|| ManagerError::operation("断开后接口从系统中消失"))
    }

    pub async fn set_device_enabled(
        &self,
        ifname: &str,
        enabled: bool,
    ) -> Result<ActionResult, ManagerError> {
        let _guard = self
            .inner
            .op_lock
            .try_lock()
            .map_err(|_| ManagerError::busy("另一个网络操作正在进行"))?;
        self.require_privilege()?;
        validate_ifname(ifname).map_err(|error| ManagerError::invalid(error.to_string()))?;
        let info = self.lookup_interface(ifname)?;
        if info.is_loopback() {
            return Err(ManagerError::invalid("不能修改回环接口"));
        }
        if !enabled {
            let device = self.disconnect_locked(&info).await?;
            return Ok(ActionResult {
                message: format!("已禁用 {ifname}"),
                device: Some(device),
            });
        }

        self.clear_error(ifname).await;
        set_link_up(&self.inner.ifconfig, ifname, true)
            .await
            .map_err(|error| ManagerError::operation(format!("启用 {ifname} 失败: {error:#}")))?;
        if info.kind() == DeviceKind::Ethernet && !self.inner.no_dhcp {
            self.set_operation(ifname, DeviceState::IpConfig).await;
            self.refresh().await;
            if let Err(error) = self.inner.dhcp.ensure(ifname, false).await {
                self.clear_operation().await;
                self.set_link_state(ifname, LinkState::Down).await;
                self.set_error(ifname, &error.to_string()).await;
                self.refresh().await;
                return Err(ManagerError::operation(format!("DHCP 配置失败: {error:#}")));
            }
            self.clear_operation().await;
            self.set_link_state(ifname, LinkState::Connected).await;
            self.inner
                .active_device
                .write()
                .await
                .replace(ifname.to_owned());
        }
        let snapshot = self.refresh().await;
        let device = snapshot
            .device(ifname)
            .cloned()
            .ok_or_else(|| ManagerError::operation("启用后接口从系统中消失"))?;
        Ok(ActionResult {
            message: format!("已启用 {ifname}"),
            device: Some(device),
        })
    }

    pub async fn set_wifi_enabled(&self, enabled: bool) -> Result<(), ManagerError> {
        let _guard = self
            .inner
            .op_lock
            .try_lock()
            .map_err(|_| ManagerError::busy("另一个网络操作正在进行"))?;
        self.require_privilege()?;
        let was_enabled = self.inner.wifi_enabled.swap(enabled, Ordering::SeqCst);
        if was_enabled == enabled {
            return Ok(());
        }
        let wireless = self
            .inner
            .wireless_ifnames
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let infos: Vec<InterfaceInfo> = system::interfaces()
            .map_err(ManagerError::from)?
            .into_iter()
            .map(|mut info| {
                info.wireless = wireless.contains(&info.ifname);
                info
            })
            .filter(|info| info.kind() == DeviceKind::Wifi)
            .collect();
        let mut first_error: Option<ManagerError> = None;
        for info in &infos {
            if enabled {
                if let Err(error) = set_link_up(&self.inner.ifconfig, &info.ifname, true).await {
                    first_error.get_or_insert_with(|| {
                        ManagerError::operation(format!("启用 {} 失败: {error:#}", info.ifname))
                    });
                }
            } else {
                self.inner.dhcp.stop(&info.ifname).await;
                if let Err(error) = self.inner.wifi.disconnect(&info.ifname).await {
                    first_error.get_or_insert_with(|| {
                        ManagerError::operation(format!("断开 {} 失败: {error:#}", info.ifname))
                    });
                }
                if let Err(error) = set_link_up(&self.inner.ifconfig, &info.ifname, false).await {
                    first_error.get_or_insert_with(|| {
                        ManagerError::operation(format!("关闭 {} 失败: {error:#}", info.ifname))
                    });
                }
            }
        }
        if !enabled {
            let mut active = self.inner.active_device.write().await;
            if active
                .as_deref()
                .is_some_and(|name| infos.iter().any(|info| info.ifname == name))
            {
                *active = None;
            }
        }
        self.refresh().await;
        if let Some(error) = first_error {
            self.inner.wifi_enabled.store(was_enabled, Ordering::SeqCst);
            return Err(error);
        }
        Ok(())
    }

    async fn resolve_security(
        &self,
        ifname: &str,
        ssid: &str,
        password: &str,
        hidden: bool,
    ) -> Security {
        let cached = self
            .inner
            .networks
            .read()
            .await
            .get(ifname)
            .and_then(|networks| {
                networks
                    .iter()
                    .find(|network| network.ssid == ssid)
                    .map(|network| network.security)
            });
        if let Some(security) = cached {
            return security;
        }
        if let Ok(networks) = self.inner.wifi.scan(ifname).await {
            let networks = deduplicate(networks);
            let security = networks
                .iter()
                .find(|network| network.ssid == ssid)
                .map(|network| network.security);
            self.inner
                .networks
                .write()
                .await
                .insert(ifname.to_owned(), networks);
            if let Some(security) = security {
                return security;
            }
        }
        if hidden {
            if password.is_empty() {
                Security::Open
            } else {
                Security::Wpa2Psk
            }
        } else if password.is_empty() {
            Security::Open
        } else {
            Security::Wpa2Psk
        }
    }

    fn lookup_interface(&self, ifname: &str) -> Result<InterfaceInfo, ManagerError> {
        let wireless = self
            .inner
            .wireless_ifnames
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        system::interfaces()
            .map_err(ManagerError::from)?
            .into_iter()
            .map(|mut info| {
                info.wireless = wireless.contains(&info.ifname);
                info
            })
            .find(|info| info.ifname == ifname)
            .ok_or_else(|| ManagerError::not_found(format!("找不到接口 {ifname}")))
    }

    fn require_wifi_interface(&self, ifname: &str) -> Result<(), ManagerError> {
        let info = self.lookup_interface(ifname)?;
        if info.kind() != DeviceKind::Wifi {
            return Err(ManagerError::invalid(format!("{ifname} 不是 Wi-Fi 接口")));
        }
        Ok(())
    }

    fn require_privilege(&self) -> Result<(), ManagerError> {
        if system::is_root() {
            Ok(())
        } else {
            Err(ManagerError::unauthorized(system::privilege_hint()))
        }
    }

    async fn set_link_state(&self, ifname: &str, state: LinkState) {
        self.inner
            .link_states
            .write()
            .await
            .insert(ifname.to_owned(), state);
    }

    async fn advance_link(&self, ifname: &str, event: LinkEvent) {
        let mut states = self.inner.link_states.write().await;
        let state = states.get(ifname).copied().unwrap_or(LinkState::Down);
        match state.transition(event) {
            Ok(next) => {
                states.insert(ifname.to_owned(), next);
            }
            Err(error) => {
                tracing::debug!(ifname, %error, "忽略无效链路状态转换");
            }
        }
    }

    async fn set_operation(&self, ifname: &str, state: DeviceState) {
        *self.inner.op_state.write().await = Some(OperationState {
            ifname: ifname.to_owned(),
            state,
        });
    }

    async fn clear_operation(&self) {
        *self.inner.op_state.write().await = None;
    }

    async fn set_error(&self, ifname: &str, message: &str) {
        self.inner
            .last_errors
            .write()
            .await
            .insert(ifname.to_owned(), message.to_owned());
    }

    async fn clear_error(&self, ifname: &str) {
        self.inner.last_errors.write().await.remove(ifname);
    }

    pub async fn shutdown(&self) {
        self.inner.dhcp.shutdown().await;
    }
}
