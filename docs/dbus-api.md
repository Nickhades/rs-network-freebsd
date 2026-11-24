# netcore D-Bus API (v1)

netcore owns the name **org.netcore.NetworkManager** on the **system bus** at
object path **/org/netcore/NetworkManager**, interface
**org.netcore.NetworkManager**. All rich payloads are UTF-8 JSON documents
defined in crates/netcore-common (Rust) and described in docs/json-schema.md.

## Why JSON strings?

The core API must be callable from Rust, GLib/GJS, QML/C++ and shell scripts.
Transporting versioned JSON documents avoids hand-maintaining a{sv}
marshalling in four languages while keeping one source of truth in
netcore-common::model. Primitive arguments are used for actions, so method
signatures stay introspectable and scriptable.

Compatibility rule: additive fields are allowed in v1. Readers must ignore
unknown JSON fields. Removing or renaming a field requires a protocol bump.

## Methods

| Method | Signature | Auth | Notes |
|---|---|---|---|
| GetSnapshot | () -> s | read | Complete ManagerSnapshot JSON. |
| ListDevices | () -> s | read | JSON array of NetworkDevice. |
| GetVersion | () -> s | read | Package version string. |
| ScanWifi | (s ifname) -> s | read | Requests a scan, waits for it, returns ScanResult JSON. |
| ConnectWifi | (s ifname, s ssid, s password, b hidden) -> s | manage | Associates and starts DHCP. Returns NetworkDevice JSON. |
| DisconnectDevice | (s ifname) -> s | manage | Stops DHCP and disconnects/unconfigures the device. Returns ActionResult JSON. |
| SetDeviceEnabled | (s ifname, b enabled) -> s | manage | Administrative up/down; enabling Ethernet starts DHCP. |
| SetWifiEnabled | (b enabled) -> () | manage | Global Wi-Fi preference; disconnects Wi-Fi when disabled. |

ScanResult/NetworkDevice JSON contains both ssid (UTF-8 display text, with
Chinese preserved) and ssid_hex (exact SSID octets). ConnectWifi accepts a
UTF-8 ssid string; non-UTF-8 SSIDs can be inspected via ssid_hex but are not
safely connectable through this string-only method (the GUI blocks them).

A failing method returns a D-Bus error whose name begins with
org.netcore.NetworkManager.Error. (for example
org.netcore.NetworkManager.Error.NotAuthorized). Messages are human-readable
and intended for direct display.

## Signals

| Signal | Signature | Meaning |
|---|---|---|
| SnapshotChanged | (s snapshot) | Any device/state change; emitted only when content changes. |
| DeviceChanged | (s device) | One changed NetworkDevice. Also covered by SnapshotChanged. |
| NetworkScanFinished | (s ifname, s networks) | A scan completed; networks is a WifiNetwork array. |
| Error | (s code, s message) | Background failure not tied to a method reply. |

## Examples

    gdbus call --system \
      --dest org.netcore.NetworkManager \
      --object-path /org/netcore/NetworkManager \
      --method org.netcore.NetworkManager.GetSnapshot

    gdbus call --system \
      --dest org.netcore.NetworkManager \
      --object-path /org/netcore/NetworkManager \
      --method org.netcore.NetworkManager.ScanWifi wlan0

## Authorization

Read methods are available to local users. Mutating methods are checked in
the daemon. On the system bus netcore asks polkit for the org.netcore.manage
action (interactive authentication permitted); root is always authorized.
On the session bus (--bus session, development only) the caller is trusted.
D-Bus policy alone is never the only security boundary.