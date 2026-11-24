//! wpa_supplicant D-Bus backend (fi.w1.wpa_supplicant1).
//!
//! Object paths are dynamic: GetInterface returns the Interface object path,
//! then every operation is scoped to it. We deliberately use the dynamic
//! Proxy API instead of generated proxies so all paths can share one code
//! path and missing optional properties degrade gracefully.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use netcore_common::ssid::display_and_hex;
use netcore_common::{Security, WifiNetwork};
use zbus::names::BusName;
use zbus::{Connection, OwnedObjectPath, OwnedVariant, Proxy, Variant};

use super::{WifiStatus, deduplicate};

const DEST: &str = "fi.w1.wpa_supplicant1";
const GLOBAL_PATH: &str = "/fi/w1/wpa_supplicant1";
const GLOBAL_IFACE: &str = "fi.w1.wpa_supplicant1";
const INTERFACE_IFACE: &str = "fi.w1.wpa_supplicant1.Interface";
const BSS_IFACE: &str = "fi.w1.wpa_supplicant1.BSS";

#[derive(Clone)]
pub struct DbusWifi {
    connection: Connection,
}

impl std::fmt::Debug for DbusWifi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbusWifi").finish_non_exhaustive()
    }
}

pub async fn owner_exists() -> bool {
    let Ok(connection) = Connection::system().await else {
        return false;
    };
    let Ok(proxy) = zbus::fdo::DBusProxy::new(&connection).await else {
        return false;
    };
    let Ok(name) = BusName::try_from(DEST) else {
        return false;
    };
    proxy.name_has_owner(name).await.unwrap_or(false)
}

impl DbusWifi {
    pub async fn new() -> Result<Self> {
        let connection = Connection::system()
            .await
            .context("无法连接系统 D-Bus 以访问 wpa_supplicant")?;
        let proxy = zbus::fdo::DBusProxy::new(&connection).await?;
        let name = BusName::try_from(DEST)?;
        if !proxy.name_has_owner(name).await.unwrap_or(false) {
            bail!("fi.w1.wpa_supplicant1 未在系统总线上运行");
        }
        Ok(Self { connection })
    }

    async fn interface(&self, ifname: &str) -> Result<Proxy<'static>> {
        let global = Proxy::new_owned(self.connection.clone(), DEST, GLOBAL_PATH, GLOBAL_IFACE)
            .await
            .context("创建 wpa_supplicant 全局代理失败")?;
        let path: OwnedObjectPath = match global.call("GetInterface", &(ifname,)).await {
            Ok(path) => path,
            Err(_) => {
                let mut args: HashMap<String, Variant<'static>> = HashMap::new();
                args.insert("Ifname".to_owned(), Variant::from(ifname.to_owned()));
                global
                    .call("CreateInterface", &(args,))
                    .await
                    .with_context(|| format!("创建 wpa_supplicant 接口 {ifname} 失败"))?
            }
        };
        Proxy::new_owned(
            self.connection.clone(),
            DEST,
            path.as_str().to_owned(),
            INTERFACE_IFACE,
        )
        .await
        .with_context(|| format!("打开 wpa_supplicant 接口对象 {path} 失败"))
    }

    pub async fn status(&self, ifname: &str) -> Result<WifiStatus> {
        let interface = self.interface(ifname).await?;
        let state = interface
            .get_property::<String>("State")
            .await
            .unwrap_or_else(|_| "unknown".to_owned());
        let connected = state.eq_ignore_ascii_case("completed");
        let current_bss = interface
            .get_property::<OwnedObjectPath>("CurrentBSS")
            .await
            .ok();
        let (ssid, ssid_hex, bssid) = if let Some(path) = current_bss
            && path.as_str() != "/"
        {
            self.bss_identity(&path).await.unwrap_or((None, None, None))
        } else {
            (None, None, None)
        };
        Ok(WifiStatus {
            connected,
            ssid,
            ssid_hex,
            bssid,
            state,
        })
    }

    pub async fn scan(&self, ifname: &str) -> Result<Vec<WifiNetwork>> {
        let interface = self.interface(ifname).await?;
        let empty: HashMap<String, Variant<'static>> = HashMap::new();
        if let Err(error) = interface.call::<_, _, ()>("Scan", &(empty,)).await {
            tracing::debug!(ifname, %error, "wpa D-Bus Scan 返回错误，尝试读取现有结果");
        }
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(300)).await;
            match interface.get_property::<bool>("Scanning").await {
                Ok(true) => continue,
                Ok(false) => break,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                    break;
                }
            }
        }

        let current = interface
            .get_property::<OwnedObjectPath>("CurrentBSS")
            .await
            .ok();
        let bss_paths = interface
            .get_property::<Vec<OwnedObjectPath>>("BSSs")
            .await
            .context("读取 wpa_supplicant BSS 列表失败")?;
        let mut networks = Vec::with_capacity(bss_paths.len());
        for path in bss_paths {
            if let Ok(Some(network)) = self.bss_network(&path, current.as_ref()).await {
                networks.push(network);
            }
        }
        Ok(deduplicate(networks))
    }

    async fn bss(&self, path: &OwnedObjectPath) -> Result<Proxy<'static>> {
        Proxy::new_owned(
            self.connection.clone(),
            DEST,
            path.as_str().to_owned(),
            BSS_IFACE,
        )
        .await
        .context("创建 wpa_supplicant BSS 代理失败")
    }

    async fn bss_identity(
        &self,
        path: &OwnedObjectPath,
    ) -> Result<(Option<String>, Option<String>, Option<String>)> {
        let bss = self.bss(path).await?;
        let raw_ssid = bss.get_property::<Vec<u8>>("SSID").await.ok();
        let (ssid, ssid_hex) = match raw_ssid {
            Some(bytes) if !bytes.is_empty() => {
                let (display, hex) = display_and_hex(&bytes);
                (Some(display), hex)
            }
            _ => (None, None),
        };
        let bssid = bss
            .get_property::<Vec<u8>>("BSSID")
            .await
            .ok()
            .map(|bytes| format_mac(&bytes));
        Ok((ssid, ssid_hex, bssid))
    }

    async fn bss_network(
        &self,
        path: &OwnedObjectPath,
        current: Option<&OwnedObjectPath>,
    ) -> Result<Option<WifiNetwork>> {
        let bss = self.bss(path).await?;
        let ssid = bss
            .get_property::<Vec<u8>>("SSID")
            .await
            .unwrap_or_default();
        if ssid.is_empty() {
            return Ok(None);
        }
        let (ssid, ssid_hex) = display_and_hex(&ssid);
        let bssid = bss
            .get_property::<Vec<u8>>("BSSID")
            .await
            .ok()
            .map(|bytes| format_mac(&bytes));
        let signal = bss.get_property::<i16>("Signal").await.unwrap_or(-100) as i32;
        let frequency = bss.get_property::<u16>("Frequency").await.unwrap_or(0) as u32;
        let privacy = bss.get_property::<bool>("Privacy").await.unwrap_or(false);
        let rsn = bss
            .get_property::<HashMap<String, OwnedVariant>>("RSN")
            .await
            .unwrap_or_default();
        let wpa = bss
            .get_property::<HashMap<String, OwnedVariant>>("WPA")
            .await
            .unwrap_or_default();
        let security = if rsn_uses_sae(&rsn) {
            Security::Wpa3Sae
        } else if !rsn.is_empty() {
            Security::Wpa2Psk
        } else if !wpa.is_empty() {
            Security::WpaPsk
        } else if privacy {
            Security::Wep
        } else {
            Security::Open
        };
        Ok(Some(WifiNetwork {
            ssid,
            ssid_hex,
            bssid,
            signal,
            frequency,
            security,
            connected: current.is_some_and(|current| current == path),
        }))
    }

    pub async fn connect(
        &self,
        ifname: &str,
        ssid: &str,
        password: &str,
        security: Security,
        hidden: bool,
    ) -> Result<()> {
        if security.requires_password() && password.is_empty() {
            bail!("该网络需要密码");
        }
        let interface = self.interface(ifname).await?;
        let _: Result<(), _> = interface.call("Disconnect", &()).await;
        let (key_mgmt, password_key) = match security {
            Security::Open => ("NONE", None),
            Security::Wpa3Sae => ("SAE", Some("sae_password")),
            Security::WpaPsk | Security::Wpa2Psk | Security::Unknown => ("WPA-PSK", Some("psk")),
            Security::Enterprise => bail!("暂不支持企业级 802.1X 网络"),
            Security::Wep => bail!("出于安全原因，netcore 不支持 WEP"),
        };
        let mut properties: HashMap<String, Variant<'static>> = HashMap::new();
        properties.insert("ssid".to_owned(), Variant::from(ssid.as_bytes().to_vec()));
        properties.insert("key_mgmt".to_owned(), Variant::from(key_mgmt.to_owned()));
        if let Some(key) = password_key {
            properties.insert(key.to_owned(), Variant::from(password.to_owned()));
        }
        if hidden {
            properties.insert("scan_ssid".to_owned(), Variant::from(true));
        }
        let network_path: OwnedObjectPath = interface
            .call("AddNetwork", &(properties,))
            .await
            .context("wpa_supplicant AddNetwork 失败")?;
        interface
            .call::<_, _, ()>("SelectNetwork", &(network_path.clone(),))
            .await
            .context("wpa_supplicant SelectNetwork 失败")?;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(35);
        loop {
            let state = interface
                .get_property::<String>("State")
                .await
                .unwrap_or_else(|_| "unknown".to_owned());
            if state.eq_ignore_ascii_case("completed") {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                let _: Result<(), _> = interface.call("RemoveNetwork", &(network_path,)).await;
                let _: Result<(), _> = interface.call("Disconnect", &()).await;
                bail!("连接超时（wpa 状态: {state}）");
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    }

    pub async fn disconnect(&self, ifname: &str) -> Result<()> {
        let interface = self.interface(ifname).await?;
        interface
            .call::<_, _, ()>("Disconnect", &())
            .await
            .context("wpa_supplicant Disconnect 失败")?;
        Ok(())
    }
}

fn format_mac(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn rsn_uses_sae(rsn: &HashMap<String, OwnedVariant>) -> bool {
    let Some(key_mgmt) = rsn.get("KeyMgmt") else {
        return false;
    };
    // KeyMgmt is an array of strings. Try a few shapes; unknown shapes simply
    // mean "not SAE", which keeps WPA2 working.
    if let Ok(value) = key_mgmt.try_clone()
        && let Ok(values) = Vec::<String>::try_from(value)
    {
        return values.iter().any(|value| value.eq_ignore_ascii_case("SAE"));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_mac() {
        assert_eq!(
            format_mac(&[0x06, 0x8b, 0x42, 0x2a, 0x17, 0xea]),
            "06:8b:42:2a:17:ea"
        );
    }

    #[test]
    fn empty_rsn_is_not_sae() {
        assert!(!rsn_uses_sae(&HashMap::new()));
    }
}
