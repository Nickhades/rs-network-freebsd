//! Async D-Bus client used by the CLI, GUI and tests.
//!
//! The daemon speaks JSON over D-Bus (see docs/dbus-api.md). This crate wraps
//! the raw proxy and parses replies into the shared model types so frontends
//! never reimplement the wire format.

use anyhow::{Context, Result};
use futures_util::Stream;
use netcore_common::{
    ActionResult, DBUS_PATH, DBUS_SERVICE, ManagerSnapshot, NetworkDevice, ScanResult,
};
use zbus::Connection;

/// Which bus a frontend should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusKind {
    System,
    Session,
}

impl BusKind {
    /// Read NETCORE_BUS, defaulting to the system bus.
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("NETCORE_BUS").as_deref() {
            Ok("session") => Self::Session,
            _ => Self::System,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Session => "session",
        }
    }
}

/// High-level netcore client.
#[derive(Debug)]
pub struct Client {
    connection: Connection,
    proxy: NetworkManagerProxy<'static>,
    bus: BusKind,
}

impl Client {
    /// Connect to the daemon and construct the typed proxy.
    ///
    /// This verifies that the daemon answers GetVersion, so an absent or
    /// unactivated service fails immediately with a useful error.
    pub async fn connect(bus: BusKind) -> Result<Self> {
        let connection = match bus {
            BusKind::System => Connection::system().await,
            BusKind::Session => Connection::session().await,
        }
        .with_context(|| format!("无法连接 {} D-Bus", bus.as_str()))?;
        let proxy = NetworkManagerProxy::builder(&connection)
            .destination(DBUS_SERVICE)
            .path(DBUS_PATH)
            .build()
            .await
            .context("无法创建 org.netcore.NetworkManager 代理")?;
        let client = Self {
            connection,
            proxy,
            bus,
        };
        client
            .version()
            .await
            .with_context(|| format!("{} 上的 netcore 守护进程不可用", bus.as_str()))?;
        Ok(client)
    }

    /// Construct a client from an existing connection (tests, embedding).
    pub async fn with_connection(connection: Connection, bus: BusKind) -> Result<Self> {
        let proxy = NetworkManagerProxy::builder(&connection)
            .destination(DBUS_SERVICE)
            .path(DBUS_PATH)
            .build()
            .await?;
        Ok(Self {
            connection,
            proxy,
            bus,
        })
    }

    #[must_use]
    pub const fn bus(&self) -> BusKind {
        self.bus
    }

    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub async fn version(&self) -> Result<String> {
        self.proxy
            .get_version()
            .await
            .context("GetVersion 调用失败")
    }

    pub async fn snapshot(&self) -> Result<ManagerSnapshot> {
        let raw = self
            .proxy
            .get_snapshot()
            .await
            .context("GetSnapshot 调用失败")?;
        serde_json::from_str(&raw).context("解析 ManagerSnapshot 失败")
    }

    pub async fn snapshot_json(&self) -> Result<String> {
        self.proxy
            .get_snapshot()
            .await
            .context("GetSnapshot 调用失败")
    }

    pub async fn devices(&self) -> Result<Vec<NetworkDevice>> {
        let raw = self
            .proxy
            .list_devices()
            .await
            .context("ListDevices 调用失败")?;
        serde_json::from_str(&raw).context("解析 NetworkDevice 列表失败")
    }

    pub async fn scan_wifi(&self, ifname: &str) -> Result<ScanResult> {
        let raw = self
            .proxy
            .scan_wifi(ifname)
            .await
            .with_context(|| format!("扫描 {ifname} 失败"))?;
        serde_json::from_str(&raw).context("解析 ScanResult 失败")
    }

    pub async fn connect_wifi(
        &self,
        ifname: &str,
        ssid: &str,
        password: &str,
        hidden: bool,
    ) -> Result<NetworkDevice> {
        let raw = self
            .proxy
            .connect_wifi(ifname, ssid, password, hidden)
            .await
            .with_context(|| format!("连接 {ssid} 失败"))?;
        serde_json::from_str(&raw).context("解析 NetworkDevice 失败")
    }

    pub async fn disconnect_device(&self, ifname: &str) -> Result<ActionResult> {
        let raw = self
            .proxy
            .disconnect_device(ifname)
            .await
            .with_context(|| format!("断开 {ifname} 失败"))?;
        serde_json::from_str(&raw).context("解析 ActionResult 失败")
    }

    pub async fn set_device_enabled(&self, ifname: &str, enabled: bool) -> Result<ActionResult> {
        let raw = self
            .proxy
            .set_device_enabled(ifname, enabled)
            .await
            .with_context(|| format!("设置 {ifname} 状态失败"))?;
        serde_json::from_str(&raw).context("解析 ActionResult 失败")
    }

    pub async fn set_wifi_enabled(&self, enabled: bool) -> Result<()> {
        self.proxy
            .set_wifi_enabled(enabled)
            .await
            .context("SetWifiEnabled 调用失败")
    }

    /// Stream of daemon snapshots, driven by the SnapshotChanged signal.
    pub async fn snapshots(&self) -> Result<impl Stream<Item = ManagerSnapshot> + '_> {
        use futures_util::StreamExt;

        let stream = self.proxy.receive_snapshot_changed().await?;
        Ok(stream.filter_map(|signal| async move {
            let args = signal.args().ok()?;
            serde_json::from_str(args.snapshot()).ok()
        }))
    }
}

#[zbus::proxy(
    interface = "org.netcore.NetworkManager",
    default_service = "org.netcore.NetworkManager",
    default_path = "/org/netcore/NetworkManager"
)]
trait NetworkManager {
    fn get_snapshot(&self) -> zbus::Result<String>;
    fn list_devices(&self) -> zbus::Result<String>;
    fn get_version(&self) -> zbus::Result<String>;
    fn scan_wifi(&self, ifname: &str) -> zbus::Result<String>;
    fn connect_wifi(
        &self,
        ifname: &str,
        ssid: &str,
        password: &str,
        hidden: bool,
    ) -> zbus::Result<String>;
    fn disconnect_device(&self, ifname: &str) -> zbus::Result<String>;
    fn set_device_enabled(&self, ifname: &str, enabled: bool) -> zbus::Result<String>;
    fn set_wifi_enabled(&self, enabled: bool) -> zbus::Result<()>;

    #[zbus(signal)]
    fn snapshot_changed(&self, snapshot: &str) -> zbus::Result<()>;
}

/// Format a zbus error for terminal/GUI display without losing the D-Bus name.
#[must_use]
pub fn describe_error(error: &zbus::Error) -> String {
    if let zbus::Error::MethodError(name, detail, _) = error {
        let short = name.as_str().rsplit('.').next().unwrap_or(name.as_str());
        if let Some(detail) = detail {
            return format!("{short}: {detail}");
        }
        return short.to_owned();
    }
    error.to_string()
}
