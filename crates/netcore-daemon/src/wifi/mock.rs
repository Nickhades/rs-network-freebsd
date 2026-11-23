//! Deterministic Wi-Fi backend for UI development and tests.

use std::sync::{Arc, Mutex};

use netcore_common::{Security, WifiNetwork};

use super::WifiStatus;

#[derive(Clone, Default)]
pub struct MockWifi {
    connected: Arc<Mutex<Option<String>>>,
}

impl MockWifi {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn status(&self, _ifname: &str) -> WifiStatus {
        let connected = self
            .connected
            .lock()
            .expect("mock wifi mutex poisoned")
            .clone();
        match connected {
            Some(ssid) => WifiStatus {
                connected: true,
                ssid: Some(ssid),
                bssid: Some("02:00:00:00:00:01".to_owned()),
                state: "COMPLETED".to_owned(),
            },
            None => WifiStatus::disconnected(),
        }
    }

    #[must_use]
    pub fn scan(&self, _ifname: &str) -> Vec<WifiNetwork> {
        vec![
            WifiNetwork {
                ssid: "netcore-demo".to_owned(),
                bssid: Some("02:00:00:00:00:01".to_owned()),
                signal: -42,
                frequency: 5180,
                security: Security::Wpa2Psk,
                connected: self
                    .connected
                    .lock()
                    .is_ok_and(|ssid| ssid.as_deref() == Some("netcore-demo")),
            },
            WifiNetwork {
                ssid: "FreeBSD-Guest".to_owned(),
                bssid: Some("02:00:00:00:00:02".to_owned()),
                signal: -67,
                frequency: 2412,
                security: Security::Open,
                connected: false,
            },
        ]
    }

    pub fn connect(&self, _ifname: &str, ssid: &str) {
        *self.connected.lock().expect("mock wifi mutex poisoned") = Some(ssid.to_owned());
    }

    pub fn disconnect(&self, _ifname: &str) {
        *self.connected.lock().expect("mock wifi mutex poisoned") = None;
    }
}
