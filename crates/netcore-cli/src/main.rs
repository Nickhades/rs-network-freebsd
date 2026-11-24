//! netcore command-line client.

use std::io::{IsTerminal, Read};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use futures_util::StreamExt;
use netcore_client::{BusKind, Client};
use netcore_common::{
    DeviceKind, DeviceState, ManagerSnapshot, NetworkDevice, ScanResult, Security, WifiNetwork,
    tools,
};

#[derive(Debug, Parser)]
#[command(
    name = "netcore",
    version,
    about = "FreeBSD 原生网络管理器命令行客户端"
)]
struct Cli {
    /// 使用 system 或 session D-Bus（也可设置 NETCORE_BUS）。
    #[arg(long, value_enum, global = true)]
    bus: Option<BusArg>,

    /// 输出机器可读的 JSON。
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BusArg {
    System,
    Session,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 显示管理器与设备的当前状态。
    Status,
    /// 列出网络设备。
    #[command(alias = "devices")]
    List,
    /// 扫描 Wi-Fi（等待扫描完成）。
    Scan {
        /// Wi-Fi 接口名；省略时使用第一个无线接口。
        ifname: Option<String>,
    },
    /// 连接一个 Wi-Fi 网络。
    Connect {
        /// Wi-Fi 接口名；省略时使用第一个无线接口。
        ifname: Option<String>,
        /// 网络名称。
        #[arg(long)]
        ssid: String,
        /// 密码。省略时从终端安全提示或 stdin 读取。
        #[arg(long)]
        password: Option<String>,
        /// 从 stdin 读取一行密码（适合脚本）。
        #[arg(long)]
        password_stdin: bool,
        /// 隐藏网络（不广播 SSID）。
        #[arg(long)]
        hidden: bool,
    },
    /// 断开一个设备。
    Disconnect {
        /// 接口名；省略时使用当前活动设备或第一个已连接设备。
        ifname: Option<String>,
    },
    /// 启用/禁用 Wi-Fi 无线电。
    Wifi {
        #[command(subcommand)]
        action: WifiAction,
    },
    /// 持续监听守护进程推送的状态。
    Monitor,
    /// 检查本机依赖、后端和权限。
    Doctor,
}

#[derive(Debug, Subcommand)]
enum WifiAction {
    On,
    Off,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(error) = run(cli).await {
        eprintln!("netcore: {error:#}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let bus = cli.bus.map_or_else(BusKind::from_env, |bus| match bus {
        BusArg::System => BusKind::System,
        BusArg::Session => BusKind::Session,
    });
    let client = Client::connect(bus).await?;
    match cli.command {
        Command::Status => {
            let snapshot = client.snapshot().await?;
            if cli.json {
                println!("{}", snapshot.to_json_pretty());
            } else {
                print_status(&snapshot);
            }
        }
        Command::List => {
            let devices = client.devices().await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&devices)?);
            } else {
                print_devices(&devices);
            }
        }
        Command::Scan { ifname } => {
            let snapshot = client.snapshot().await?;
            let ifname = wifi_ifname(&snapshot, ifname.as_deref())?;
            let result = client.scan_wifi(&ifname).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print_scan(&result);
            }
        }
        Command::Connect {
            ifname,
            ssid,
            password,
            password_stdin,
            hidden,
        } => {
            let snapshot = client.snapshot().await?;
            let ifname = wifi_ifname(&snapshot, ifname.as_deref())?;
            let mut password = password;
            if password.is_none() && !password_stdin {
                let cached = snapshot.device(&ifname).and_then(|device| {
                    device
                        .networks
                        .iter()
                        .find(|network| network.ssid == ssid)
                        .map(|network| network.security)
                });
                let security = if cached.is_none() && !hidden {
                    let scan = client.scan_wifi(&ifname).await?;
                    scan.networks
                        .iter()
                        .find(|network| network.ssid == ssid)
                        .map(|network| network.security)
                } else {
                    cached
                };
                if security.is_some_and(Security::requires_password) {
                    if std::io::stdin().is_terminal() {
                        password = Some(
                            rpassword::prompt_password(format!("{ssid} 的密码: "))
                                .context("读取密码失败")?,
                        );
                    } else {
                        bail!("该网络需要密码；请使用 --password 或 --password-stdin");
                    }
                }
            }
            if password_stdin {
                let mut buffer = String::new();
                std::io::stdin()
                    .read_to_string(&mut buffer)
                    .context("从 stdin 读取密码失败")?;
                password = Some(buffer.trim_end_matches(['\r', '\n']).to_owned());
            }
            let device = client
                .connect_wifi(&ifname, &ssid, password.as_deref().unwrap_or(""), hidden)
                .await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&device)?);
            } else {
                println!(
                    "已连接 {ssid} ({ifname})，IPv4: {}",
                    device.primary_ipv4().unwrap_or("等待 DHCP")
                );
            }
        }
        Command::Disconnect { ifname } => {
            let snapshot = client.snapshot().await?;
            let ifname = disconnect_ifname(&snapshot, ifname.as_deref())?;
            let result = client.disconnect_device(&ifname).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", result.message);
            }
        }
        Command::Wifi { action } => {
            let enabled = matches!(action, WifiAction::On);
            client.set_wifi_enabled(enabled).await?;
            if cli.json {
                println!("{{\"wifi_enabled\":{enabled}}}");
            } else {
                println!("Wi-Fi 已{}", if enabled { "启用" } else { "禁用" });
            }
        }
        Command::Monitor => {
            monitor(&client, cli.json).await?;
        }
        Command::Doctor => {
            doctor(&client, bus).await?;
        }
    }
    Ok(())
}

fn wifi_ifname(snapshot: &ManagerSnapshot, requested: Option<&str>) -> Result<String> {
    if let Some(ifname) = requested {
        let device = snapshot
            .device(ifname)
            .with_context(|| format!("找不到接口 {ifname}"))?;
        if device.kind != DeviceKind::Wifi {
            bail!("{ifname} 不是 Wi-Fi 接口");
        }
        return Ok(ifname.to_owned());
    }
    snapshot
        .devices
        .iter()
        .find(|device| device.kind == DeviceKind::Wifi)
        .map(|device| device.ifname.clone())
        .context("没有可用的 Wi-Fi 接口")
}

fn disconnect_ifname(snapshot: &ManagerSnapshot, requested: Option<&str>) -> Result<String> {
    if let Some(ifname) = requested {
        if snapshot.device(ifname).is_none() {
            bail!("找不到接口 {ifname}");
        }
        return Ok(ifname.to_owned());
    }
    if let Some(active) = &snapshot.active_device
        && snapshot.device(active).is_some()
    {
        return Ok(active.clone());
    }
    snapshot
        .devices
        .iter()
        .find(|device| device.state == DeviceState::Connected)
        .map(|device| device.ifname.clone())
        .context("没有已连接的设备")
}

fn print_status(snapshot: &ManagerSnapshot) {
    println!(
        "netcore {}  {}  状态: {} ({})",
        snapshot.version,
        snapshot.hostname,
        snapshot.state.label(),
        snapshot.connectivity.label()
    );
    println!(
        "Wi-Fi: {}  后端: {}  Netlink: {}{}",
        if snapshot.wifi_enabled { "开" } else { "关" },
        snapshot.wpa_backend,
        if snapshot.netlink_active {
            "是"
        } else {
            "否（轮询）"
        },
        snapshot
            .active_device
            .as_ref()
            .map(|name| format!("  活动设备: {name}"))
            .unwrap_or_default()
    );
    println!();
    print_devices(&snapshot.devices);
    if let Some(device) = snapshot
        .devices
        .iter()
        .find(|device| device.kind == DeviceKind::Wifi && !device.networks.is_empty())
    {
        println!();
        println!("Wi-Fi 网络 ({}):", device.ifname);
        print_networks(&device.networks);
    }
}

fn print_devices(devices: &[NetworkDevice]) {
    if devices.is_empty() {
        println!("没有网络设备");
        return;
    }
    for device in devices {
        let marker = match device.state {
            DeviceState::Connected => "●",
            DeviceState::Connecting | DeviceState::IpConfig => "◐",
            DeviceState::Failed => "✕",
            _ => "○",
        };
        let connection = match device.kind {
            DeviceKind::Wifi => device.ssid.clone().unwrap_or_else(|| "未关联".to_owned()),
            _ => device.primary_ipv4().unwrap_or("无地址").to_owned(),
        };
        println!(
            "{marker} {:<8} {:<6} {:<10} {}",
            device.ifname,
            device.kind.label(),
            device.state.label(),
            connection
        );
        if !device.ipv4.is_empty() || !device.ipv6.is_empty() {
            println!("    {}", device.ipv4.join(", "));
            if !device.ipv6.is_empty() {
                println!("    {}", device.ipv6.join(", "));
            }
        }
        if let Some(mac) = &device.mac {
            println!(
                "    MAC {mac}  MTU {}",
                device
                    .mtu
                    .map_or_else(|| "-".to_owned(), |mtu| mtu.to_string())
            );
        }
        if let Some(error) = &device.last_error {
            println!("    错误: {error}");
        }
    }
}

fn print_scan(result: &ScanResult) {
    println!("{} 扫描到 {} 个网络:", result.ifname, result.networks.len());
    print_networks(&result.networks);
}

fn print_networks(networks: &[WifiNetwork]) {
    if networks.is_empty() {
        println!("  （无结果，请稍后重试）");
        return;
    }
    for network in networks {
        println!(
            "  {} {:>3}%  {:<8} {}{}",
            signal_bar(network.quality_percent()),
            network.quality_percent(),
            network.security.label(),
            network.ssid,
            if network.connected {
                "  [已连接]"
            } else {
                ""
            }
        );
    }
}

fn signal_bar(quality: u8) -> &'static str {
    match quality {
        0..=20 => "▂___",
        21..=40 => "▂▄__",
        41..=60 => "▂▄▆_",
        61..=80 => "▂▄▆█",
        _ => "▂▄▆█",
    }
}

async fn monitor(client: &Client, json: bool) -> Result<()> {
    let initial = client.snapshot().await?;
    if json {
        println!("{}", initial.to_json());
    } else {
        print_status_summary(&initial);
    }
    let stream = client.snapshots().await?;
    tokio::pin!(stream);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            snapshot = stream.next() => {
                match snapshot {
                    Some(snapshot) if json => println!("{}", snapshot.to_json()),
                    Some(snapshot) => print_status_summary(&snapshot),
                    None => {
                        bail!("守护进程信号流已关闭");
                    }
                }
            }
        }
    }
    Ok(())
}

fn print_status_summary(snapshot: &ManagerSnapshot) {
    let active = snapshot
        .devices
        .iter()
        .filter(|device| device.state == DeviceState::Connected)
        .map(|device| match device.kind {
            DeviceKind::Wifi => format!(
                "{}:{}",
                device.ifname,
                device.ssid.as_deref().unwrap_or("-")
            ),
            _ => format!("{}:{}", device.ifname, device.primary_ipv4().unwrap_or("-")),
        })
        .collect::<Vec<_>>()
        .join("  ");
    println!(
        "[{}] {} / {} / Wi-Fi {} / {}",
        snapshot.updated_at,
        snapshot.state.label(),
        snapshot.connectivity.label(),
        if snapshot.wifi_enabled { "开" } else { "关" },
        if active.is_empty() {
            "无活动连接"
        } else {
            &active
        }
    );
}

async fn doctor(client: &Client, bus: BusKind) -> Result<()> {
    println!("netcore doctor");
    println!(
        "  操作系统: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    if let Some(warning) = rtw89_kernel_warning() {
        println!("  内核驱动风险: {warning}");
    }
    println!("  D-Bus: {}", bus.as_str());
    let snapshot = client.snapshot().await?;
    println!(
        "  守护进程: {} (后端 {})",
        snapshot.version, snapshot.wpa_backend
    );
    println!(
        "  Netlink: {}",
        if snapshot.netlink_active {
            "可用"
        } else {
            "不可用（轮询）"
        }
    );
    println!(
        "  Wi-Fi 硬件: {}",
        if snapshot.wifi_available {
            "可用"
        } else {
            "不可用"
        }
    );
    println!(
        "  权限: {}",
        if is_root() {
            "root（可修改网络）"
        } else {
            "普通用户（只读；修改需要 root 或 polkit）"
        }
    );
    let dhcpcd = tools::dhcpcd();
    for (label, found) in [
        ("ifconfig", tools::ifconfig()),
        ("dhcpcd", dhcpcd.clone()),
        ("wpa_cli", tools::wpa_cli()),
    ] {
        match found {
            Some(path) => println!("  {label}: {}（存在）", path.display()),
            None => println!("  {label}: 未找到"),
        }
    }
    if dhcpcd.is_some() {
        let running = std::path::Path::new("/var/run/dhcpcd/pid").exists();
        println!(
            "  dhcpcd 运行状态: {}",
            if running { "运行中" } else { "未运行" }
        );
    }

    let wifi_ifnames: Vec<&str> = snapshot
        .devices
        .iter()
        .filter(|device| device.kind == DeviceKind::Wifi)
        .map(|device| device.ifname.as_str())
        .collect();
    if wifi_ifnames.is_empty() {
        println!("  wpa_supplicant 控制套接字: 未发现 Wi-Fi 接口");
    } else if let Some(wpa_cli) = tools::wpa_cli() {
        let mut state = "不可访问";
        for ifname in wifi_ifnames {
            let output = ProcessCommand::new(&wpa_cli)
                .args(["-i", ifname, "status"])
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .output();
            if output.is_ok_and(|output| output.status.success()) {
                state = "可访问";
                break;
            }
        }
        println!(
            "  wpa_supplicant 控制套接字: {state}（{}）",
            wpa_cli.display()
        );
    } else {
        println!("  wpa_supplicant 控制套接字: wpa_cli 不可执行");
    }
    if snapshot.devices.is_empty() {
        bail!("系统没有可管理的网络接口");
    }
    Ok(())
}

/// FreeBSD 15.1's rtw89 driver has a known use-after-free/page-fault bug in
/// the TX path (FreeBSD PR 285228).  The fix landed on stable/15; keep the
/// warning until the running kernel is at least the 15.2 branch.
const RTW89_FIXED_FREEBSD_VERSION: u64 = 1_502_000;

fn rtw89_warning(freebsd_version: u64, driver: Option<&str>) -> Option<&'static str> {
    if freebsd_version >= RTW89_FIXED_FREEBSD_VERSION
        || !driver.is_some_and(|driver| driver.trim() == "rtw89")
    {
        return None;
    }
    Some(
        "检测到 FreeBSD 15.1 的 rtw89 无线驱动；该版本存在发送路径内核 panic（FreeBSD PR 285228）。请升级到包含修复的 15-STABLE/15.2 内核，或在升级前关闭 Wi-Fi 并使用有线网络。",
    )
}

fn rtw89_kernel_warning() -> Option<&'static str> {
    let version_output = ProcessCommand::new("uname").arg("-K").output().ok()?;
    if !version_output.status.success() {
        return None;
    }
    let freebsd_version = String::from_utf8_lossy(&version_output.stdout)
        .trim()
        .parse::<u64>()
        .ok()?;

    let sysctl = tools::sysctl()?;
    let driver_output = ProcessCommand::new(sysctl)
        .args(["-n", "dev.rtw89.0.%driver"])
        .output()
        .ok()?;
    if !driver_output.status.success() {
        return None;
    }
    let driver = String::from_utf8_lossy(&driver_output.stdout);
    rtw89_warning(freebsd_version, Some(driver.trim()))
}

#[cfg(unix)]
fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
fn is_root() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtw89_warning_only_on_affected_kernels() {
        assert!(rtw89_warning(1_501_000, Some("rtw89")).is_some());
        assert!(rtw89_warning(1_502_000, Some("rtw89")).is_none());
        assert!(rtw89_warning(1_501_000, Some("iwlwifi")).is_none());
        assert!(rtw89_warning(1_501_000, None).is_none());
    }

    #[test]
    fn signal_bar_has_four_slots() {
        for quality in 0..=100 {
            assert_eq!(signal_bar(quality).chars().count(), 4);
        }
    }

    #[test]
    fn target_selection_prefers_wifi() {
        let snapshot = ManagerSnapshot {
            protocol_version: 1,
            version: "0".to_owned(),
            hostname: "test".to_owned(),
            state: Default::default(),
            connectivity: Default::default(),
            wifi_enabled: true,
            wifi_available: true,
            wpa_backend: "mock".to_owned(),
            netlink_active: false,
            updated_at: 0,
            devices: vec![
                NetworkDevice {
                    ifname: "ue0".to_owned(),
                    kind: DeviceKind::Ethernet,
                    state: DeviceState::Connected,
                    managed: true,
                    enabled: true,
                    carrier: true,
                    mac: None,
                    mtu: None,
                    ipv4: vec![],
                    ipv6: vec![],
                    ssid: None,
                    ssid_hex: None,
                    networks: vec![],
                    last_error: None,
                },
                NetworkDevice {
                    ifname: "wlan0".to_owned(),
                    kind: DeviceKind::Wifi,
                    state: DeviceState::Disconnected,
                    managed: true,
                    enabled: true,
                    carrier: false,
                    mac: None,
                    mtu: None,
                    ipv4: vec![],
                    ipv6: vec![],
                    ssid: None,
                    ssid_hex: None,
                    networks: vec![],
                    last_error: None,
                },
            ],
            active_device: Some("ue0".to_owned()),
        };
        assert_eq!(wifi_ifname(&snapshot, None).unwrap(), "wlan0");
        assert_eq!(disconnect_ifname(&snapshot, None).unwrap(), "ue0");
    }
}
