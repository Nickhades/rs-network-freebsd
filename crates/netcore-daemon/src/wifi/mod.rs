//! Wi-Fi backend abstraction.
//!
//! FreeBSD's ports build of wpa_supplicant frequently has the control socket
//! interface enabled but not the D-Bus interface. netcore therefore supports
//! both and selects one at startup (auto mode). Both return the same typed
//! model to the manager.

mod ctrl;
mod dbus;
mod mock;

use std::path::PathBuf;

use anyhow::{Result, bail};
use netcore_common::{Security, WifiNetwork};

use crate::config::WpaChoice;

pub use mock::MockWifi;

#[derive(Debug, Clone)]
pub struct WifiStatus {
    pub connected: bool,
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub state: String,
}

impl WifiStatus {
    #[must_use]
    pub fn disconnected() -> Self {
        Self {
            connected: false,
            ssid: None,
            bssid: None,
            state: "disconnected".to_owned(),
        }
    }
}

#[derive(Clone)]
enum Backend {
    Dbus(dbus::DbusWifi),
    Ctrl(ctrl::CtrlWifi),
    Mock(MockWifi),
    Unavailable(String),
}

/// Wi-Fi operations for all wireless interfaces.
#[derive(Clone)]
pub struct WifiService {
    backend: Backend,
}

impl std::fmt::Debug for WifiService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WifiService")
            .field("backend", &self.kind())
            .finish()
    }
}

impl WifiService {
    /// Pick the requested backend, or the best available one in auto mode.
    pub async fn detect(choice: WpaChoice, ctrl_dir: PathBuf, wpa_cli: PathBuf) -> Self {
        let ctrl = ctrl::CtrlWifi::new(ctrl_dir.clone(), wpa_cli.clone());
        match choice {
            WpaChoice::Mock => Self {
                backend: Backend::Mock(MockWifi::new()),
            },
            WpaChoice::Ctrl => Self {
                backend: Backend::Ctrl(ctrl),
            },
            WpaChoice::Dbus => match dbus::DbusWifi::new().await {
                Ok(backend) => Self {
                    backend: Backend::Dbus(backend),
                },
                Err(error) => Self {
                    backend: Backend::Unavailable(format!("D-Bus 后端不可用: {error}")),
                },
            },
            WpaChoice::Auto => {
                if dbus::owner_exists().await {
                    match dbus::DbusWifi::new().await {
                        Ok(backend) => {
                            tracing::info!("wpa_supplicant 后端: D-Bus");
                            return Self {
                                backend: Backend::Dbus(backend),
                            };
                        }
                        Err(error) => {
                            tracing::warn!(%error, "wpa_supplicant D-Bus 可发现但连接失败，尝试控制套接字");
                        }
                    }
                }
                if ctrl.usable() {
                    tracing::info!("wpa_supplicant 后端: 控制套接字");
                    Self {
                        backend: Backend::Ctrl(ctrl),
                    }
                } else {
                    tracing::warn!("未发现可用的 wpa_supplicant 后端");
                    Self {
                        backend: Backend::Unavailable(
                            "未发现 wpa_supplicant D-Bus 服务或控制套接字".to_owned(),
                        ),
                    }
                }
            }
        }
    }

    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self.backend {
            Backend::Dbus(_) => "dbus",
            Backend::Ctrl(_) => "ctrl",
            Backend::Mock(_) => "mock",
            Backend::Unavailable(_) => "unavailable",
        }
    }

    #[must_use]
    pub fn available(&self) -> bool {
        !matches!(self.backend, Backend::Unavailable(_))
    }

    pub async fn status(&self, ifname: &str) -> Result<WifiStatus> {
        match &self.backend {
            Backend::Dbus(backend) => backend.status(ifname).await,
            Backend::Ctrl(backend) => backend.status(ifname).await,
            Backend::Mock(backend) => Ok(backend.status(ifname)),
            Backend::Unavailable(reason) => bail!("{reason}"),
        }
    }

    pub async fn scan(&self, ifname: &str) -> Result<Vec<WifiNetwork>> {
        match &self.backend {
            Backend::Dbus(backend) => backend.scan(ifname).await,
            Backend::Ctrl(backend) => backend.scan(ifname).await,
            Backend::Mock(backend) => Ok(backend.scan(ifname)),
            Backend::Unavailable(reason) => bail!("{reason}"),
        }
    }

    pub async fn connect(
        &self,
        ifname: &str,
        ssid: &str,
        password: &str,
        security: Security,
        hidden: bool,
    ) -> Result<()> {
        match &self.backend {
            Backend::Dbus(backend) => {
                backend
                    .connect(ifname, ssid, password, security, hidden)
                    .await
            }
            Backend::Ctrl(backend) => {
                backend
                    .connect(ifname, ssid, password, security, hidden)
                    .await
            }
            Backend::Mock(backend) => {
                backend.connect(ifname, ssid);
                Ok(())
            }
            Backend::Unavailable(reason) => bail!("{reason}"),
        }
    }

    pub async fn disconnect(&self, ifname: &str) -> Result<()> {
        match &self.backend {
            Backend::Dbus(backend) => backend.disconnect(ifname).await,
            Backend::Ctrl(backend) => backend.disconnect(ifname).await,
            Backend::Mock(backend) => {
                backend.disconnect(ifname);
                Ok(())
            }
            Backend::Unavailable(reason) => bail!("{reason}"),
        }
    }
}

/// Infer security from wpa_supplicant's textual flags
/// (for example "[WPA2-PSK-CCMP][ESS]").
#[must_use]
pub fn security_from_flags(flags: &str) -> Security {
    let upper = flags.to_ascii_uppercase();
    if upper.contains("EAP") || upper.contains("802.1X") || upper.contains("IEEE8021X") {
        Security::Enterprise
    } else if upper.contains("SAE") || upper.contains("WPA3") {
        Security::Wpa3Sae
    } else if upper.contains("WPA2") {
        Security::Wpa2Psk
    } else if upper.contains("WPA") {
        Security::WpaPsk
    } else if upper.contains("WEP") || upper.contains("PRIVACY") {
        Security::Wep
    } else if upper.contains("ESS") || flags.trim().is_empty() {
        Security::Open
    } else {
        Security::Unknown
    }
}

/// Decode the backslash escapes wpa_cli uses for non-printable SSIDs.
#[must_use]
pub fn unescape_wpa(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 1 < bytes.len() {
            match bytes[index + 1] {
                b'x' | b'X' if index + 3 < bytes.len() => {
                    let hex = &input[index + 2..index + 4];
                    if let Ok(value) = u8::from_str_radix(hex, 16) {
                        output.push(value);
                        index += 4;
                        continue;
                    }
                }
                b'n' => {
                    output.push(b'\n');
                    index += 2;
                    continue;
                }
                b'r' => {
                    output.push(b'\r');
                    index += 2;
                    continue;
                }
                b't' => {
                    output.push(b'\t');
                    index += 2;
                    continue;
                }
                b'\\' => {
                    output.push(b'\\');
                    index += 2;
                    continue;
                }
                b'"' => {
                    output.push(b'"');
                    index += 2;
                    continue;
                }
                other => {
                    output.push(other);
                    index += 2;
                    continue;
                }
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output)
        .trim_end_matches('\0')
        .to_owned()
}

/// Escape a value for the wpa_supplicant control protocol. The caller adds
/// surrounding double quotes where a string value is expected.
#[must_use]
pub fn escape_wpa_value(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'"' => output.push_str("\\\""),
            b'\\' => output.push_str("\\\\"),
            b'\n' => output.push_str("\\n"),
            b'\r' => output.push_str("\\r"),
            b'\t' => output.push_str("\\t"),
            0x20..=0x7e => output.push(*byte as char),
            other => output.push_str(&format!("\\x{other:02x}")),
        }
    }
    output
}

/// Collapse multiple BSS entries for one SSID, keeping the strongest.
#[must_use]
pub fn deduplicate(mut networks: Vec<WifiNetwork>) -> Vec<WifiNetwork> {
    networks.sort_by(|left, right| {
        right
            .signal
            .cmp(&left.signal)
            .then_with(|| left.ssid.cmp(&right.ssid))
    });
    let mut result: Vec<WifiNetwork> = Vec::new();
    for network in networks {
        if let Some(existing) = result.iter_mut().find(|item| item.ssid == network.ssid) {
            existing.connected |= network.connected;
            if network.signal > existing.signal {
                existing.signal = network.signal;
                existing.bssid = network.bssid;
                existing.frequency = network.frequency;
                existing.security = network.security;
            }
        } else {
            result.push(network);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(ssid: &str, signal: i32) -> WifiNetwork {
        WifiNetwork {
            ssid: ssid.to_owned(),
            bssid: None,
            signal,
            frequency: 2412,
            security: Security::Open,
            connected: false,
        }
    }

    #[test]
    fn classifies_security_flags() {
        assert_eq!(
            security_from_flags("[WPA2-PSK-CCMP][ESS]"),
            Security::Wpa2Psk
        );
        assert_eq!(
            security_from_flags("[WPA3-SAE-CCMP][ESS]"),
            Security::Wpa3Sae
        );
        assert_eq!(security_from_flags("[WPA-PSK-TKIP][ESS]"), Security::WpaPsk);
        assert_eq!(
            security_from_flags("[WPA2-EAP-CCMP][ESS]"),
            Security::Enterprise
        );
        assert_eq!(security_from_flags("[ESS]"), Security::Open);
        assert_eq!(security_from_flags("[WEP][ESS]"), Security::Wep);
    }

    #[test]
    fn unescapes_utf8_ssid() {
        assert_eq!(unescape_wpa("\\xe4\\xba\\x91\\xe6\\xb6\\xa7"), "云涧");
        assert_eq!(unescape_wpa("hello"), "hello");
        assert_eq!(unescape_wpa("a\\tb"), "a\tb");
    }

    #[test]
    fn escapes_control_and_quotes() {
        assert_eq!(escape_wpa_value("a\"b"), "a\\\"b");
        assert_eq!(escape_wpa_value("a\\b"), "a\\\\b");
        assert_eq!(escape_wpa_value("a\nb"), "a\\nb");
    }

    #[test]
    fn deduplication_keeps_strongest() {
        let mut weak = network("net", -80);
        weak.bssid = Some("00:00:00:00:00:01".to_owned());
        let mut strong = network("net", -40);
        strong.bssid = Some("00:00:00:00:00:02".to_owned());
        strong.connected = true;
        let result = deduplicate(vec![weak, strong]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].signal, -40);
        assert!(result[0].connected);
    }
}
