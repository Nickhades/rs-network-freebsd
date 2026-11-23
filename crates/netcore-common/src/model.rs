use serde::{Deserialize, Serialize};

/// The kind of network interface as presented to users.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Wifi,
    Ethernet,
    Loopback,
    #[default]
    Other,
}

impl DeviceKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Wifi => "Wi-Fi",
            Self::Ethernet => "以太网",
            Self::Loopback => "回环",
            Self::Other => "其他",
        }
    }

    #[must_use]
    pub const fn icon_name(self) -> &'static str {
        match self {
            Self::Wifi => "network-wireless-symbolic",
            Self::Ethernet => "network-wired-symbolic",
            Self::Loopback => "network-loopback-symbolic",
            Self::Other => "network-transmit-receive-symbolic",
        }
    }
}

/// Lifecycle state of one device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    #[default]
    Unknown,
    Unmanaged,
    Unavailable,
    Disconnected,
    Connecting,
    IpConfig,
    Connected,
    Disconnecting,
    Failed,
}

impl DeviceState {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "未知",
            Self::Unmanaged => "未托管",
            Self::Unavailable => "不可用",
            Self::Disconnected => "未连接",
            Self::Connecting => "正在连接",
            Self::IpConfig => "正在获取地址",
            Self::Connected => "已连接",
            Self::Disconnecting => "正在断开",
            Self::Failed => "连接失败",
        }
    }

    #[must_use]
    pub const fn is_transitioning(self) -> bool {
        matches!(
            self,
            Self::Connecting | Self::IpConfig | Self::Disconnecting
        )
    }

    #[must_use]
    pub const fn is_connected(self) -> bool {
        matches!(self, Self::Connected)
    }
}

/// Security mode inferred from wpa_supplicant BSS flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Security {
    Open,
    Wep,
    WpaPsk,
    Wpa2Psk,
    Wpa3Sae,
    Enterprise,
    #[default]
    Unknown,
}

impl Security {
    #[must_use]
    pub const fn requires_password(self) -> bool {
        !matches!(self, Self::Open | Self::Unknown)
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Open => "开放",
            Self::Wep => "WEP",
            Self::WpaPsk => "WPA",
            Self::Wpa2Psk => "WPA2",
            Self::Wpa3Sae => "WPA3",
            Self::Enterprise => "企业级",
            Self::Unknown => "未知",
        }
    }
}

/// One visible Wi-Fi network, collapsed across BSSIDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiNetwork {
    pub ssid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bssid: Option<String>,
    /// Signal strength in dBm (typically -95..=-20).
    #[serde(default)]
    pub signal: i32,
    /// Center frequency in MHz.
    #[serde(default)]
    pub frequency: u32,
    #[serde(default)]
    pub security: Security,
    #[serde(default)]
    pub connected: bool,
}

impl WifiNetwork {
    /// Convert dBm to a user-facing 0-100 quality percentage.
    #[must_use]
    pub fn quality_percent(&self) -> u8 {
        let clamped = self.signal.clamp(-100, -50);
        (((clamped + 100) * 2).clamp(0, 100)) as u8
    }
}

/// One interface in the system snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkDevice {
    pub ifname: String,
    #[serde(default)]
    pub kind: DeviceKind,
    #[serde(default)]
    pub state: DeviceState,
    #[serde(default)]
    pub managed: bool,
    /// Administrative up flag (IFF_UP).
    #[serde(default)]
    pub enabled: bool,
    /// Physical link / association (IFF_RUNNING).
    #[serde(default)]
    pub carrier: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u32>,
    /// IPv4 addresses in CIDR notation.
    #[serde(default)]
    pub ipv4: Vec<String>,
    /// IPv6 addresses in CIDR notation.
    #[serde(default)]
    pub ipv6: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    /// Cached scan results for Wi-Fi devices.
    #[serde(default)]
    pub networks: Vec<WifiNetwork>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl NetworkDevice {
    #[must_use]
    pub fn primary_ipv4(&self) -> Option<&str> {
        self.ipv4
            .first()
            .map(|ip| ip.split('/').next().unwrap_or(ip))
    }

    #[must_use]
    pub fn display_name(&self) -> String {
        match self.kind {
            DeviceKind::Wifi => self
                .ssid
                .as_deref()
                .filter(|ssid| !ssid.is_empty())
                .map_or_else(
                    || format!("Wi-Fi ({})", self.ifname),
                    |ssid| ssid.to_owned(),
                ),
            _ => self.ifname.clone(),
        }
    }
}

/// Overall manager state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerState {
    #[default]
    Offline,
    Connecting,
    Connected,
    Disconnecting,
}

impl ManagerState {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Offline => "离线",
            Self::Connecting => "正在连接",
            Self::Connected => "已连接",
            Self::Disconnecting => "正在断开",
        }
    }
}

/// Coarse connectivity level. netcore deliberately does not probe the
/// internet; "local" means at least one usable address is configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Connectivity {
    #[default]
    None,
    Local,
    Unknown,
}

impl Connectivity {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "无连接",
            Self::Local => "本地网络",
            Self::Unknown => "未知",
        }
    }
}

/// Complete daemon snapshot pushed to every frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagerSnapshot {
    /// Wire protocol version.
    #[serde(default)]
    pub protocol_version: u32,
    /// netcore package version.
    pub version: String,
    pub hostname: String,
    #[serde(default)]
    pub state: ManagerState,
    #[serde(default)]
    pub connectivity: Connectivity,
    #[serde(default)]
    pub wifi_enabled: bool,
    #[serde(default)]
    pub wifi_available: bool,
    /// "dbus", "ctrl", "unavailable" or "mock".
    #[serde(default)]
    pub wpa_backend: String,
    #[serde(default)]
    pub netlink_active: bool,
    /// Unix timestamp in seconds.
    #[serde(default)]
    pub updated_at: u64,
    pub devices: Vec<NetworkDevice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_device: Option<String>,
}

impl ManagerSnapshot {
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("ManagerSnapshot is always serializable")
    }

    #[must_use]
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("ManagerSnapshot is always serializable")
    }

    #[must_use]
    pub fn device(&self, ifname: &str) -> Option<&NetworkDevice> {
        self.devices.iter().find(|device| device.ifname == ifname)
    }
}

/// Request body accepted by the CLI and reusable by other clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectWifiRequest {
    pub ifname: String,
    pub ssid: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub hidden: bool,
}

/// Result of an operation that may return a fresh device object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionResult {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<NetworkDevice>,
}

/// Result of an explicit scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResult {
    pub ifname: String,
    pub networks: Vec<WifiNetwork>,
    /// Unix timestamp in seconds.
    #[serde(default)]
    pub scanned_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trips_through_json() {
        let snapshot = ManagerSnapshot {
            protocol_version: crate::PROTOCOL_VERSION,
            version: crate::VERSION.to_owned(),
            hostname: "freebsd".to_owned(),
            state: ManagerState::Connected,
            connectivity: Connectivity::Local,
            wifi_enabled: true,
            wifi_available: true,
            wpa_backend: "ctrl".to_owned(),
            netlink_active: true,
            updated_at: 1,
            devices: vec![NetworkDevice {
                ifname: "wlan0".to_owned(),
                kind: DeviceKind::Wifi,
                state: DeviceState::Connected,
                managed: true,
                enabled: true,
                carrier: true,
                mac: Some("00:11:22:33:44:55".to_owned()),
                mtu: Some(1500),
                ipv4: vec!["192.0.2.10/24".to_owned()],
                ipv6: vec![],
                ssid: Some("test".to_owned()),
                networks: vec![WifiNetwork {
                    ssid: "test".to_owned(),
                    bssid: Some("00:11:22:33:44:55".to_owned()),
                    signal: -52,
                    frequency: 2412,
                    security: Security::Wpa2Psk,
                    connected: true,
                }],
                last_error: None,
            }],
            active_device: Some("wlan0".to_owned()),
        };
        let json = snapshot.to_json();
        let decoded: ManagerSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snapshot, decoded);
    }

    #[test]
    fn quality_is_clamped() {
        let mut network = WifiNetwork {
            ssid: String::new(),
            bssid: None,
            signal: -42,
            frequency: 0,
            security: Security::Open,
            connected: false,
        };
        assert_eq!(network.quality_percent(), 100);
        network.signal = -120;
        assert_eq!(network.quality_percent(), 0);
        network.signal = -75;
        assert_eq!(network.quality_percent(), 50);
    }

    #[test]
    fn security_password_rules() {
        assert_eq!(Security::default(), Security::Unknown);
        assert!(!Security::Open.requires_password());
        assert!(Security::Wpa2Psk.requires_password());
        assert!(Security::Enterprise.requires_password());
    }
}
