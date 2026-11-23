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

    /// Directory holding wpa_supplicant control sockets.
    #[arg(long, default_value = "/var/run/wpa_supplicant")]
    pub wpa_ctrl_dir: PathBuf,

    /// Path to wpa_cli.
    #[arg(long, default_value = "/usr/sbin/wpa_cli")]
    pub wpa_cli: PathBuf,

    /// Path to dhclient.
    #[arg(long, default_value = "/sbin/dhclient")]
    pub dhclient: PathBuf,

    /// Path to ifconfig.
    #[arg(long, default_value = "/sbin/ifconfig")]
    pub ifconfig: PathBuf,

    /// DHCP client timeout in seconds.
    #[arg(long, default_value_t = 45)]
    pub dhcp_timeout: u64,

    /// Do not start dhclient; use statically configured addresses.
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
