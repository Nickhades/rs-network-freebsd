//! Shared protocol and model types for netcore.
//!
//! The daemon, CLI, GUI and desktop shell integrations all speak the same
//! JSON documents defined here. The D-Bus surface deliberately transports
//! these documents as strings so it stays easy to consume from JavaScript,
//! QML and shell scripts while the Rust sides still use strongly typed data.

pub mod error;
pub mod model;
pub mod ssid;
pub mod tools;

pub use error::{ErrorCode, ErrorPayload};
pub use model::{
    ActionResult, ConnectWifiRequest, Connectivity, DeviceKind, DeviceState, ManagerSnapshot,
    ManagerState, NetworkDevice, ScanResult, Security, WifiNetwork,
};

/// Well-known D-Bus coordinates of the netcore daemon.
pub const DBUS_SERVICE: &str = "org.netcore.NetworkManager";
pub const DBUS_PATH: &str = "/org/netcore/NetworkManager";
pub const DBUS_INTERFACE: &str = "org.netcore.NetworkManager";

/// Polkit action used for privileged operations.
pub const POLKIT_ACTION: &str = "org.netcore.manage";

/// Version of the wire protocol spoken by this build.
pub const PROTOCOL_VERSION: u32 = 1;

/// Human-readable package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
