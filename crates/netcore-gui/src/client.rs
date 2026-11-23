//! D-Bus worker thread for the GTK frontend.
//!
//! GTK widgets stay on the main thread. A dedicated thread owns a Tokio
//! runtime and the zbus connection; UI requests go in through a Tokio mpsc
//! channel and snapshots come back through a std channel polled by the GLib
//! main loop.

use std::sync::mpsc::Sender;
use std::time::Duration;

use netcore_client::{BusKind, Client};
use netcore_common::ManagerSnapshot;
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Debug)]
pub enum Request {
    Refresh,
    Scan {
        ifname: String,
    },
    ConnectWifi {
        ifname: String,
        ssid: String,
        password: String,
        hidden: bool,
    },
    Disconnect {
        ifname: String,
    },
    SetDeviceEnabled {
        ifname: String,
        enabled: bool,
    },
    SetWifiEnabled {
        enabled: bool,
    },
}

#[derive(Debug)]
pub enum UiEvent {
    Connecting,
    Snapshot(Box<ManagerSnapshot>),
    DaemonUnavailable(String),
    Busy(bool),
    ActionFinished(String),
    Error(String),
}

pub fn spawn(bus: BusKind, request_rx: UnboundedReceiver<Request>, event_tx: Sender<UiEvent>) {
    let builder = std::thread::Builder::new().name("netcore-dbus".to_owned());
    let _ = builder.spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = event_tx.send(UiEvent::DaemonUnavailable(format!(
                    "无法创建异步运行时: {error}"
                )));
                return;
            }
        };
        runtime.block_on(run(bus, request_rx, event_tx));
    });
}

async fn run(bus: BusKind, mut request_rx: UnboundedReceiver<Request>, event_tx: Sender<UiEvent>) {
    loop {
        let _ = event_tx.send(UiEvent::Connecting);
        match Client::connect(bus).await {
            Ok(client) => {
                if !run_session(client, &mut request_rx, &event_tx).await {
                    return;
                }
            }
            Err(error) => {
                let _ = event_tx.send(UiEvent::DaemonUnavailable(format!("{error:#}")));
                tokio::time::sleep(Duration::from_secs(2)).await;
                if request_rx.is_closed() {
                    return;
                }
            }
        }
    }
}

/// Returns false when the UI side has gone away.
async fn run_session(
    client: Client,
    request_rx: &mut UnboundedReceiver<Request>,
    event_tx: &Sender<UiEvent>,
) -> bool {
    match client.snapshot().await {
        Ok(snapshot) => {
            let _ = event_tx.send(UiEvent::Snapshot(Box::new(snapshot)));
        }
        Err(error) => {
            let _ = event_tx.send(UiEvent::DaemonUnavailable(format!("{error:#}")));
            return true;
        }
    }

    let mut ticker = tokio::time::interval(Duration::from_secs(3));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                match client.snapshot().await {
                    Ok(snapshot) => {
                        let _ = event_tx.send(UiEvent::Snapshot(Box::new(snapshot)));
                    }
                    Err(error) => {
                        let _ = event_tx.send(UiEvent::DaemonUnavailable(format!("{error:#}")));
                        return true;
                    }
                }
            }
            request = request_rx.recv() => {
                let Some(request) = request else {
                    return false;
                };
                let needs_busy = !matches!(request, Request::Refresh);
                if needs_busy {
                    let _ = event_tx.send(UiEvent::Busy(true));
                }
                let message = handle_request(&client, request).await;
                match message {
                    Ok(message) => {
                        if let Some(message) = message {
                            let _ = event_tx.send(UiEvent::ActionFinished(message));
                        }
                    }
                    Err(error) => {
                        let _ = event_tx.send(UiEvent::Error(format!("{error:#}")));
                    }
                }
                if needs_busy {
                    let _ = event_tx.send(UiEvent::Busy(false));
                }
                match client.snapshot().await {
                    Ok(snapshot) => {
                        let _ = event_tx.send(UiEvent::Snapshot(Box::new(snapshot)));
                    }
                    Err(error) => {
                        let _ = event_tx.send(UiEvent::DaemonUnavailable(format!("{error:#}")));
                        return true;
                    }
                }
            }
        }
    }
}

async fn handle_request(client: &Client, request: Request) -> anyhow::Result<Option<String>> {
    match request {
        Request::Refresh => Ok(None),
        Request::Scan { ifname } => {
            let result = client.scan_wifi(&ifname).await?;
            Ok(Some(format!(
                "扫描完成，发现 {} 个网络",
                result.networks.len()
            )))
        }
        Request::ConnectWifi {
            ifname,
            ssid,
            password,
            hidden,
        } => {
            let device = client
                .connect_wifi(&ifname, &ssid, &password, hidden)
                .await?;
            Ok(Some(format!(
                "已连接 {ssid} ({ifname})，IP: {}",
                device.primary_ipv4().unwrap_or("等待 DHCP")
            )))
        }
        Request::Disconnect { ifname } => {
            let result = client.disconnect_device(&ifname).await?;
            Ok(Some(result.message))
        }
        Request::SetDeviceEnabled { ifname, enabled } => {
            let result = client.set_device_enabled(&ifname, enabled).await?;
            Ok(Some(result.message))
        }
        Request::SetWifiEnabled { enabled } => {
            client.set_wifi_enabled(enabled).await?;
            Ok(Some(format!(
                "Wi-Fi 已{}",
                if enabled { "启用" } else { "禁用" }
            )))
        }
    }
}
