# JSON documents (protocol v1)

All documents are UTF-8 JSON objects. Unknown fields must be ignored.

## ManagerSnapshot

    {
      "protocol_version": 1,
      "version": "0.1.0",
      "hostname": "BSD",
      "state": "connected",
      "connectivity": "local",
      "wifi_enabled": true,
      "wifi_available": true,
      "wpa_backend": "ctrl",
      "netlink_active": true,
      "updated_at": 1763924000,
      "active_device": "wlan0",
      "devices": [NetworkDevice]
    }

state is one of: offline, connecting, connected, disconnecting.
connectivity is one of: none, local, unknown.

## NetworkDevice

    {
      "ifname": "wlan0",
      "kind": "wifi",
      "state": "connected",
      "managed": true,
      "enabled": true,
      "carrier": true,
      "mac": "b8:1e:a4:a3:29:e9",
      "mtu": 1500,
      "ipv4": ["192.168.2.226/24"],
      "ipv6": ["fe80::1/64"],
      "ssid": "example",
      "networks": [WifiNetwork],
      "last_error": null
    }

kind is one of: wifi, ethernet, loopback, other.
state is one of: unknown, unmanaged, unavailable, disconnected, connecting,
ip_config, connected, disconnecting, failed.

## WifiNetwork

    {
      "ssid": "example",
      "bssid": "06:8b:42:2a:17:ea",
      "signal": -52,
      "frequency": 2412,
      "security": "wpa2_psk",
      "connected": true
    }

security is one of: open, wep, wpa_psk, wpa2_psk, wpa3_sae, enterprise,
unknown.

## ScanResult and ActionResult

    {"ifname":"wlan0","networks":[],"scanned_at":1763924000}
    {"message":"已连接到 example","device":{...}}
