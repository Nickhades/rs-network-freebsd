use std::path::PathBuf;

use clap::{Parser, ValueEnum};

/// netcore: native network manager for FreeBSD.
#[derive(Debug, Clone, Parser)]
#[command(name = "netcore-daemon", version, about)]
pub struct Config {
    /// D-Bus to serve on. The session bus is for development only.
    #[arg(long, value_enum, default_value_t = BusChoice::System)]
    pub bus: BusChoice,

    /// wpa_supplicant integration backend.
    #[arg(long, value_enum, default_value_t = WpaChoice::Auto)]
    pub wpa_backend: WpaChoice,

    /// Directory holding wpa_supplicant control sockets. Default = auto-detect.
    #[arg(long)]
    pub wpa_ctrl_dir: Option<PathBuf>,

    /// Path to wpa_cli. Default = search PATH and common FreeBSD locations.
    #[arg(long)]
    pub wpa_cli: Option<PathBuf>,

    /// DHCP backend. Auto prefers dhcpcd, then dhclient.
    #[arg(long, value_enum, default_value_t = DhcpChoice::Auto)]
    pub dhcp_backend: DhcpChoice,

    /// Path to dhclient. Default = search PATH and common FreeBSD locations.
    #[arg(long)]
    pub dhclient: Option<PathBuf>,

    /// Path to dhcpcd. Default = search PATH and /usr/local/sbin.
    #[arg(long)]
    pub dhcpcd: Option<PathBuf>,

    /// Path to ifconfig. Default = search PATH and common FreeBSD locations.
    #[arg(long)]
    pub ifconfig: Option<PathBuf>,

    /// DHCP client timeout in seconds.
    #[arg(long, default_value_t = 45)]
    pub dhcp_timeout: u64,

    /// Do not start a DHCP client; use statically configured addresses.
    #[arg(long)]
    pub no_dhcp: bool,

    /// Fallback polling interval in seconds when Netlink is unavailable.
    #[arg(long, default_value_t = 8)]
    pub poll_interval: u64,

    /// Run the Netlink parser in a Capsicum-isolated child process.
    #[arg(long)]
    pub capsicum: bool,

    /// Authorization mode for privileged methods.
    #[arg(long, value_enum, default_value_t = AuthChoice::Auto)]
    pub auth: AuthChoice,

    /// FreeBSD group whose members may perform network changes.
    #[arg(long, default_value = "wheel")]
    pub allow_group: String,

    /// Log filter (overrides NETCORE_LOG).
    #[arg(long)]
    pub log: Option<String>,

    /// Internal: run the Capsicum Netlink helper on stdout.
    #[arg(long, hide = true)]
    pub netlink_helper: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BusChoice {
    System,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DhcpChoice {
    Auto,
    Dhclient,
    Dhcpcd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WpaChoice {
    Auto,
    Dbus,
    Ctrl,
    Mock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AuthChoice {
    Auto,
    None,
    Group,
    Polkit,
}

impl Config {
    #[must_use]
    pub fn log_filter(&self) -> String {
        self.log
            .clone()
            .or_else(|| std::env::var("NETCORE_LOG").ok())
            .unwrap_or_else(|| "netcore_daemon=info,zbus=warn".to_owned())
    }
}
