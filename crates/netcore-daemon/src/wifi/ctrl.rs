//! wpa_supplicant control-socket backend through wpa_cli.
//!
//! This is the portable fallback for FreeBSD builds where wpa_supplicant has
//! no D-Bus interface compiled in. Commands are executed without a shell and
//! every dynamic value is escaped for wpa_supplicant's quoted-string parser.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use netcore_common::ssid::{display_and_hex, same_ssid};
use netcore_common::{Security, WifiNetwork};
use tokio::process::Command;

use super::{WifiStatus, deduplicate, escape_wpa_value, security_from_flags, unescape_wpa_bytes};

struct NetworkParams<'a> {
    ssid: &'a str,
    password: &'a str,
    key_mgmt: &'a str,
    password_key: Option<&'a str>,
    hidden: bool,
}

#[derive(Clone)]
pub struct CtrlWifi {
    ctrl_dir: PathBuf,
    wpa_cli: PathBuf,
}

impl CtrlWifi {
    #[must_use]
    pub fn new(ctrl_dir: PathBuf, wpa_cli: PathBuf) -> Self {
        Self { ctrl_dir, wpa_cli }
    }

    #[must_use]
    pub fn usable(&self) -> bool {
        self.wpa_cli.is_file() && (self.ctrl_dir.as_os_str().is_empty() || self.ctrl_dir.is_dir())
    }

    async fn run_bytes(&self, ifname: &str, args: &[&str]) -> Result<Vec<u8>> {
        let mut command = Command::new(&self.wpa_cli);
        if !self.ctrl_dir.as_os_str().is_empty() {
            command.arg("-p").arg(&self.ctrl_dir);
        }
        let output = tokio::time::timeout(
            Duration::from_secs(20),
            command
                .arg("-i")
                .arg(ifname)
                .args(args)
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .output(),
        )
        .await
        .with_context(|| format!("wpa_cli {} 超时", args.join(" ")))?
        .with_context(|| format!("无法执行 {}", self.wpa_cli.display()))?;
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "wpa_cli 失败 ({}): {}",
                output.status,
                if stderr.is_empty() {
                    stdout.trim().to_owned()
                } else {
                    stderr
                }
            );
        }
        let trimmed = trim_ascii(&output.stdout);
        if trimmed.starts_with(b"FAIL") {
            bail!(
                "wpa_supplicant: {}",
                String::from_utf8_lossy(trimmed).trim()
            );
        }
        Ok(output.stdout)
    }

    async fn run(&self, ifname: &str, args: &[&str]) -> Result<String> {
        let stdout = self.run_bytes(ifname, args).await?;
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    }

    async fn exec_values(&self, ifname: &str, args: &[String]) -> Result<String> {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(ifname, &refs).await
    }

    pub async fn status(&self, ifname: &str) -> Result<WifiStatus> {
        let raw = self.run_bytes(ifname, &["status"]).await?;
        Ok(parse_status(&raw))
    }

    pub async fn scan(&self, ifname: &str) -> Result<Vec<WifiNetwork>> {
        let scan_result = self.run(ifname, &["scan"]).await;
        if let Err(error) = &scan_result {
            let message = error.to_string();
            if !message.contains("FAIL-BUSY") && !message.contains("Command failed") {
                return Err(scan_result.unwrap_err());
            }
            tracing::debug!(ifname, "扫描已在进行中，复用当前结果");
        }
        tokio::time::sleep(Duration::from_millis(2200)).await;
        let status = self.status(ifname).await.ok();
        let raw = self.run_bytes(ifname, &["scan_results"]).await?;
        let mut networks = parse_scan_results(&raw);
        if let Some(status) = status {
            for network in &mut networks {
                network.connected = status.connected
                    && (same_ssid(
                        status.ssid.as_deref().unwrap_or_default(),
                        status.ssid_hex.as_deref(),
                        &network.ssid,
                        network.ssid_hex.as_deref(),
                    ) || (status.bssid.is_some() && status.bssid == network.bssid));
            }
        }
        Ok(deduplicate(networks))
    }

    pub async fn connect(
        &self,
        ifname: &str,
        ssid: &str,
        password: &str,
        security: Security,
        hidden: bool,
    ) -> Result<()> {
        let (key_mgmt, password_key) = match security {
            Security::Open => ("NONE", None),
            Security::Wpa3Sae => ("SAE", Some("sae_password")),
            Security::WpaPsk | Security::Wpa2Psk | Security::Unknown => ("WPA-PSK", Some("psk")),
            Security::Enterprise => {
                bail!("暂不支持企业级 802.1X 网络；请使用 wpa_supplicant 配置文件")
            }
            Security::Wep => bail!("出于安全原因，netcore 不支持 WEP"),
        };
        if security.requires_password() && password.is_empty() {
            bail!("该网络需要密码");
        }

        let _ = self.disconnect(ifname).await;
        let id = self.add_network(ifname).await?;
        let result = self
            .configure_and_select(
                ifname,
                &id,
                NetworkParams {
                    ssid,
                    password,
                    key_mgmt,
                    password_key,
                    hidden,
                },
            )
            .await;
        if let Err(error) = result {
            let _ = self.remove_network(ifname, &id).await;
            return Err(error);
        }
        match self.wait_completed(ifname).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = self.remove_network(ifname, &id).await;
                let _ = self.disconnect(ifname).await;
                Err(error)
            }
        }
    }

    async fn add_network(&self, ifname: &str) -> Result<String> {
        let raw = self.run(ifname, &["add_network"]).await?;
        let id = raw.trim();
        if id.is_empty() || !id.chars().all(|ch| ch.is_ascii_digit()) {
            bail!("wpa_supplicant 返回了无效的网络 id: {id:?}");
        }
        Ok(id.to_owned())
    }

    async fn configure_and_select(
        &self,
        ifname: &str,
        id: &str,
        params: NetworkParams<'_>,
    ) -> Result<()> {
        let quoted_ssid = format!("\"{}\"", escape_wpa_value(params.ssid));
        self.set(ifname, id, "ssid", &quoted_ssid).await?;
        self.set(ifname, id, "key_mgmt", params.key_mgmt).await?;
        if let Some(key) = params.password_key {
            let quoted_password = format!("\"{}\"", escape_wpa_value(params.password));
            self.set(ifname, id, key, &quoted_password).await?;
        }
        if params.hidden {
            self.set(ifname, id, "scan_ssid", "1").await?;
        }
        self.run(ifname, &["enable_network", id]).await?;
        self.run(ifname, &["select_network", id]).await?;
        Ok(())
    }

    async fn set(&self, ifname: &str, id: &str, key: &str, value: &str) -> Result<()> {
        let args = vec![
            "set_network".to_owned(),
            id.to_owned(),
            key.to_owned(),
            value.to_owned(),
        ];
        self.exec_values(ifname, &args).await?;
        Ok(())
    }

    async fn remove_network(&self, ifname: &str, id: &str) -> Result<()> {
        self.run(ifname, &["remove_network", id]).await?;
        Ok(())
    }

    async fn wait_completed(&self, ifname: &str) -> Result<()> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(35);
        let mut last = String::from("unknown");
        loop {
            match self.status(ifname).await {
                Ok(status) => {
                    last.clone_from(&status.state);
                    if status.connected {
                        return Ok(());
                    }
                }
                Err(error) => {
                    tracing::debug!(ifname, %error, "读取 wpa 状态失败");
                }
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("连接超时（wpa_state={last}）");
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub async fn disconnect(&self, ifname: &str) -> Result<()> {
        match self.run(ifname, &["disconnect"]).await {
            Ok(_) => Ok(()),
            Err(error) => {
                // A disconnected interface legitimately returns FAIL here.
                tracing::debug!(ifname, %error, "wpa disconnect 返回错误，已忽略");
                Ok(())
            }
        }
    }
}

fn parse_status(raw: &[u8]) -> WifiStatus {
    let mut state = "UNKNOWN".to_owned();
    let mut ssid = None;
    let mut ssid_hex = None;
    let mut bssid = None;

    for line in raw.split(|byte| *byte == b'\n') {
        let line = strip_cr(line);
        let Some(separator) = line.iter().position(|byte| *byte == b'=') else {
            continue;
        };
        let key = &line[..separator];
        let value = &line[separator + 1..];
        match key {
            b"wpa_state" => {
                state = String::from_utf8_lossy(trim_ascii(value)).into_owned();
            }
            b"ssid" => {
                let decoded = unescape_wpa_bytes(value);
                if !decoded.is_empty() {
                    let (display, hex) = display_and_hex(&decoded);
                    ssid = Some(display);
                    ssid_hex = hex;
                }
            }
            b"bssid" => {
                let text = String::from_utf8_lossy(trim_ascii(value)).into_owned();
                if !text.is_empty() {
                    bssid = Some(text.to_ascii_lowercase());
                }
            }
            _ => {}
        }
    }

    WifiStatus {
        connected: state.eq_ignore_ascii_case("COMPLETED"),
        ssid,
        ssid_hex,
        bssid,
        state,
    }
}

fn parse_scan_results(raw: &[u8]) -> Vec<WifiNetwork> {
    let mut networks = Vec::new();
    for line in raw.split(|byte| *byte == b'\n') {
        let line = strip_cr(line);
        if line.is_empty() || line.starts_with(b"bssid /") {
            continue;
        }
        // Split only the four ASCII columns. Everything after the fourth tab
        // is the SSID, so literal tabs are not silently discarded.
        let fields: Vec<&[u8]> = line.splitn(5, |byte| *byte == b'\t').collect();
        if fields.len() != 5 {
            continue;
        }
        let bssid = String::from_utf8_lossy(trim_ascii(fields[0]))
            .trim()
            .to_ascii_lowercase();
        let Ok(frequency) = std::str::from_utf8(trim_ascii(fields[1]))
            .unwrap_or_default()
            .parse::<u32>()
        else {
            continue;
        };
        let Ok(signal) = std::str::from_utf8(trim_ascii(fields[2]))
            .unwrap_or_default()
            .parse::<i32>()
        else {
            continue;
        };
        let flags = std::str::from_utf8(fields[3]).unwrap_or_default();
        let ssid_bytes = unescape_wpa_bytes(fields[4]);
        if ssid_bytes.is_empty() {
            continue;
        }
        let (ssid, ssid_hex) = display_and_hex(&ssid_bytes);
        networks.push(WifiNetwork {
            ssid,
            ssid_hex,
            bssid: Some(bssid),
            signal,
            frequency,
            security: security_from_flags(flags),
            connected: false,
        });
    }
    networks
}

fn strip_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(|byte| byte.is_ascii_whitespace()) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(|byte| byte.is_ascii_whitespace()) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status() {
        let status = parse_status(
            b"bssid=06:8b:42:2a:17:ea\nfreq=2412\nssid=\\xe4\\xba\\x91\\xe6\\xb6\\xa7  \nkey_mgmt=WPA2-PSK\nwpa_state=COMPLETED\n",
        );
        assert!(status.connected);
        assert_eq!(status.ssid.as_deref(), Some("云涧  "));
        assert_eq!(status.ssid_hex.as_deref(), Some("e4ba91e6b6a72020"));
    }

    #[test]
    fn parses_scan_results() {
        let raw = b"bssid / frequency / signal level / flags / ssid\n06:8b:42:2a:17:ea\t2412\t-37\t[WPA2-PSK-CCMP][ESS]\t\\xe4\\xba\\x91\\xe6\\xb6\\xa7\nff:ff:ff:ff:ff:ff\t5180\t-70\t[ESS]\tGuest\n";
        let networks = parse_scan_results(raw);
        assert_eq!(networks.len(), 2);
        assert_eq!(networks[0].ssid, "云涧");
        assert_eq!(networks[0].ssid_hex.as_deref(), Some("e4ba91e6b6a7"));
        assert_eq!(networks[0].security, Security::Wpa2Psk);
        assert_eq!(networks[1].security, Security::Open);
    }

    #[test]
    fn keeps_spaces_and_tabs_in_ssid() {
        let raw = b"00:11:22:33:44:55\t2412\t-40\t[ESS]\t  spaced\tname  \n";
        let networks = parse_scan_results(raw);
        assert_eq!(networks.len(), 1);
        assert_eq!(networks[0].ssid, "  spaced\tname  ");
        assert_eq!(
            networks[0].ssid_hex.as_deref(),
            Some("2020737061636564096e616d652020")
        );
    }

    #[test]
    fn preserves_non_utf8_ssid_bytes() {
        let raw = b"00:11:22:33:44:55\t2412\t-40\t[ESS]\t\\xff\\x00a\n";
        let networks = parse_scan_results(raw);
        assert_eq!(networks.len(), 1);
        assert_eq!(networks[0].ssid, "\\xff\\x00\\x61");
        assert_eq!(networks[0].ssid_hex.as_deref(), Some("ff0061"));
    }
}
