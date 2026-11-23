//! org.netcore.NetworkManager service and signal fan-out.

use std::collections::HashMap;

use netcore_common::{
    DBUS_INTERFACE, DBUS_PATH, DBUS_SERVICE, ErrorCode, ManagerSnapshot, NetworkDevice, VERSION,
};
use zbus::fdo::RequestNameFlags;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::{Connection, interface};

use crate::auth::Authorizer;
use crate::error::ManagerError;
use crate::manager::{Manager, ManagerEvent};

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.netcore.NetworkManager.Error")]
pub enum ServiceError {
    #[zbus(error)]
    ZBus(zbus::Error),
    InvalidArgument(String),
    NotAuthorized(String),
    NotFound(String),
    BackendUnavailable(String),
    Busy(String),
    OperationFailed(String),
    Unsupported(String),
}

impl From<ManagerError> for ServiceError {
    fn from(error: ManagerError) -> Self {
        let message = error.message;
        match error.code {
            ErrorCode::InvalidArgument => Self::InvalidArgument(message),
            ErrorCode::NotAuthorized => Self::NotAuthorized(message),
            ErrorCode::NotFound => Self::NotFound(message),
            ErrorCode::BackendUnavailable => Self::BackendUnavailable(message),
            ErrorCode::Busy => Self::Busy(message),
            ErrorCode::OperationFailed => Self::OperationFailed(message),
            ErrorCode::Unsupported => Self::Unsupported(message),
        }
    }
}

pub struct DaemonService {
    manager: Manager,
    authorizer: Authorizer,
}

impl DaemonService {
    #[must_use]
    pub fn new(manager: Manager, authorizer: Authorizer) -> Self {
        Self {
            manager,
            authorizer,
        }
    }

    async fn authorize(
        &self,
        connection: &Connection,
        header: &Header<'_>,
    ) -> Result<(), ServiceError> {
        self.authorizer
            .authorize(connection, header)
            .await
            .map_err(ServiceError::from)
    }
}

#[interface(name = "org.netcore.NetworkManager")]
impl DaemonService {
    #[zbus(name = "GetSnapshot")]
    async fn get_snapshot(&self) -> String {
        self.manager.refresh().await.to_json()
    }

    #[zbus(name = "ListDevices")]
    async fn list_devices(&self) -> String {
        serde_json::to_string(&self.manager.refresh().await.devices)
            .expect("NetworkDevice list is serializable")
    }

    #[zbus(name = "GetVersion")]
    fn get_version(&self) -> String {
        VERSION.to_owned()
    }

    #[zbus(name = "ScanWifi")]
    async fn scan_wifi(&self, ifname: &str) -> Result<String, ServiceError> {
        let result = self.manager.scan_wifi(ifname).await?;
        Ok(serde_json::to_string(&result).expect("ScanResult is serializable"))
    }

    #[zbus(name = "ConnectWifi")]
    async fn connect_wifi(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
        ifname: &str,
        ssid: &str,
        password: &str,
        hidden: bool,
    ) -> Result<String, ServiceError> {
        self.authorize(connection, &header).await?;
        let device = self
            .manager
            .connect_wifi(ifname, ssid, password, hidden)
            .await?;
        Ok(serde_json::to_string(&device).expect("NetworkDevice is serializable"))
    }

    #[zbus(name = "DisconnectDevice")]
    async fn disconnect_device(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
        ifname: &str,
    ) -> Result<String, ServiceError> {
        self.authorize(connection, &header).await?;
        let result = self.manager.disconnect_device(ifname).await?;
        Ok(serde_json::to_string(&result).expect("ActionResult is serializable"))
    }

    #[zbus(name = "SetDeviceEnabled")]
    async fn set_device_enabled(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
        ifname: &str,
        enabled: bool,
    ) -> Result<String, ServiceError> {
        self.authorize(connection, &header).await?;
        let result = self.manager.set_device_enabled(ifname, enabled).await?;
        Ok(serde_json::to_string(&result).expect("ActionResult is serializable"))
    }

    #[zbus(name = "SetWifiEnabled")]
    async fn set_wifi_enabled(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
        enabled: bool,
    ) -> Result<(), ServiceError> {
        self.authorize(connection, &header).await?;
        self.manager.set_wifi_enabled(enabled).await?;
        Ok(())
    }

    #[zbus(signal)]
    async fn snapshot_changed(emitter: &SignalEmitter<'_>, snapshot: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn device_changed(emitter: &SignalEmitter<'_>, device: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn network_scan_finished(
        emitter: &SignalEmitter<'_>,
        ifname: &str,
        networks: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal, name = "Error")]
    async fn error_signal(
        emitter: &SignalEmitter<'_>,
        code: &str,
        message: &str,
    ) -> zbus::Result<()>;
}

/// Forward manager events to D-Bus signals, deduplicating per-device changes.
pub async fn signal_pump(
    connection: Connection,
    mut receiver: tokio::sync::broadcast::Receiver<ManagerEvent>,
) {
    let emitter = match SignalEmitter::new(&connection, DBUS_PATH) {
        Ok(emitter) => emitter.into_owned(),
        Err(error) => {
            tracing::error!(%error, "无法创建 D-Bus 信号发送器");
            return;
        }
    };
    let mut previous_devices: HashMap<String, NetworkDevice> = HashMap::new();
    loop {
        let event = match receiver.recv().await {
            Ok(event) => event,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "D-Bus 信号接收器落后，丢弃旧事件");
                continue;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        };
        match event {
            ManagerEvent::Snapshot(snapshot) => {
                let json = snapshot.to_json();
                if let Err(error) = emitter
                    .emit(DBUS_INTERFACE, "SnapshotChanged", &(json,))
                    .await
                {
                    tracing::warn!(%error, "发送 SnapshotChanged 失败");
                }
                emit_device_changes(&emitter, &snapshot, &mut previous_devices).await;
            }
            ManagerEvent::ScanFinished { ifname, networks } => {
                let networks =
                    serde_json::to_string(&networks).expect("WifiNetwork list is serializable");
                if let Err(error) = emitter
                    .emit(DBUS_INTERFACE, "NetworkScanFinished", &(ifname, networks))
                    .await
                {
                    tracing::warn!(%error, "发送 NetworkScanFinished 失败");
                }
            }
            ManagerEvent::Error { code, message } => {
                let code = code.as_str().to_owned();
                if let Err(error) = emitter
                    .emit(DBUS_INTERFACE, "Error", &(code, message))
                    .await
                {
                    tracing::warn!(%error, "发送 Error 信号失败");
                }
            }
        }
    }
}

async fn emit_device_changes(
    emitter: &SignalEmitter<'static>,
    snapshot: &ManagerSnapshot,
    previous: &mut HashMap<String, NetworkDevice>,
) {
    let mut current = HashMap::with_capacity(snapshot.devices.len());
    for device in &snapshot.devices {
        current.insert(device.ifname.clone(), device.clone());
        if previous.get(&device.ifname) != Some(device) {
            let json = serde_json::to_string(device).expect("NetworkDevice is serializable");
            if let Err(error) = emitter
                .emit(DBUS_INTERFACE, "DeviceChanged", &(json,))
                .await
            {
                tracing::warn!(%error, "发送 DeviceChanged 失败");
            }
        }
    }
    previous.retain(|ifname, _| current.contains_key(ifname));
    *previous = current;
}

/// Acquire the well-known name without queueing behind another daemon.
pub async fn acquire_name(connection: &Connection) -> anyhow::Result<()> {
    use anyhow::{Context, bail};
    let reply = connection
        .request_name_with_flags(DBUS_SERVICE, RequestNameFlags::DO_NOT_QUEUE)
        .await
        .context("请求 D-Bus 名称失败")?;
    if !matches!(
        reply,
        zbus::fdo::RequestNameReply::PrimaryOwner | zbus::fdo::RequestNameReply::AlreadyOwner
    ) {
        bail!("D-Bus 名称 {DBUS_SERVICE} 已被另一个进程占用（{reply:?}）");
    }
    Ok(())
}
