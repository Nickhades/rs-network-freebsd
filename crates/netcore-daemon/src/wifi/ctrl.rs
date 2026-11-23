//! wpa_supplicant control-socket backend through wpa_cli.
//!
//! This is the portable fallback for FreeBSD builds where wpa_supplicant has
//! no D-Bus interface compiled in. Commands are executed without a shell and
//! every dynamic value is escaped for wpa_supplicant's quoted-string parser.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use netcore_common::{Security, WifiNetwork};
use tokio::process::Command;

use super::{WifiStatus, deduplicate, escape_wpa_value, security_from_flags, unescape_wpa};

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
        if !self.wpa_cli.is_file() {
            return false;
        }
        self.ctrl_dir.is_dir()
    }

    async fn run(&self, ifname: &str, args: &[&str]) -> Result<String> {
        let output = tokio::time::timeout(
            Duration::from_secs(20),
            Command::new(&self.wpa_cli)
                .arg("-i")
                .arg(ifname)
                .args(args)
                .env("LC_ALL", "C")
                .output(),
        )
        .await
        .with_context(|| format!("wpa_cli {} 超时", args.join(" ")))?
        .with_context(|| format!("无法执行 {}", self.wpa_cli.display()))?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if !output.status.success() {
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
        let trimmed = stdout.trim();
        if trimmed.starts_with("FAIL") {
            bail!("wpa_supplicant: {trimmed}");
        }
        Ok(stdout)
    }

    async fn exec_values(&self, ifname: &str, args: &[String]) -> Result<String> {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(ifname, &refs).await
    }

    pub async fn status(&self, ifname: &str) -> Result<WifiStatus> {
        let raw = self.run(ifname, &["status"]).await?;
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
        let raw = self.run(ifname, &["scan_results"]).await?;
        let mut networks = parse_scan_results(&raw);
        if let Some(status) = status {
            for network in &mut networks {
                network.connected = status.connected
                    && (status.ssid.as_deref() == Some(network.ssid.as_str())
                        || (status.bssid.is_some() && status.bssid == network.bssid));
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

fn parse_status(raw: &str) -> WifiStatus {
    let mut fields = std::collections::HashMap::new();
    for line in raw.lines() {
        if let Some((key, value)) = line.split_once('=') {
            fields.insert(key.trim(), value.trim().to_owned());
        }
    }
    let state = fields
        .get("wpa_state")
        .cloned()
        .unwrap_or_else(|| "UNKNOWN".to_owned());
    let ssid = fields
        .get("ssid")
        .map(|value| unescape_wpa(value))
        .filter(|value| !value.is_empty());
    let bssid = fields.get("bssid").cloned();
    WifiStatus {
        connected: state.eq_ignore_ascii_case("COMPLETED"),
        ssid,
        bssid,
        state,
    }
}

fn parse_scan_results(raw: &str) -> Vec<WifiNetwork> {
    let mut networks = Vec::new();
    for line in raw.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with("bssid /") {
            continue;
        }
        let mut fields = line.split('\t');
        let Some(bssid) = fields.next() else {
            continue;
        };
        let Some(frequency) = fields
            .next()
            .and_then(|value| value.trim().parse::<u32>().ok())
        else {
            continue;
        };
        let Some(signal) = fields
            .next()
            .and_then(|value| value.trim().parse::<i32>().ok())
        else {
            continue;
        };
        let Some(flags) = fields.next() else {
            continue;
        };
        let Some(ssid_raw) = fields.next() else {
            continue;
        };
        let ssid = unescape_wpa(ssid_raw);
        if ssid.trim().is_empty() {
            continue;
        }
        networks.push(WifiNetwork {
            ssid,
            bssid: Some(bssid.trim().to_ascii_lowercase()),
            signal,
            frequency,
            security: security_from_flags(flags),
            connected: false,
        });
    }
    networks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status() {
        let status = parse_status(
            "bssid=06:8b:42:2a:17:ea\nfreq=2412\nssid=\\xe4\\xba\\x91\nkey_mgmt=WPA2-PSK\nwpa_state=COMPLETED\n",
        );
        assert!(status.connected);
        assert_eq!(status.ssid.as_deref(), Some("云"));
    }

    #[test]
    fn parses_scan_results() {
        let raw = "bssid / frequency / signal level / flags / ssid\n06:8b:42:2a:17:ea\t2412\t-37\t[WPA2-PSK-CCMP][ESS]\t\\xe4\\xba\\x91\\xe6\\xb6\\xa7\nff:ff:ff:ff:ff:ff\t5180\t-70\t[ESS]\tGuest\n";
        let networks = parse_scan_results(raw);
        assert_eq!(networks.len(), 2);
        assert_eq!(networks[0].ssid, "云涧");
        assert_eq!(networks[0].security, Security::Wpa2Psk);
        assert_eq!(networks[1].security, Security::Open);
    }
}
