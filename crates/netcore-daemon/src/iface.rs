//! Interface enumeration through getifaddrs(3), including FreeBSD if_data.

use std::collections::BTreeMap;
use std::ffi::CStr;
use std::fmt::Write as _;
use std::net::{Ipv4Addr, Ipv6Addr};

use anyhow::{Context, Result};
use netcore_common::{DeviceKind, DeviceState, NetworkDevice};

/// Raw facts about one interface before manager policy is applied.
#[derive(Debug, Clone, Default)]
pub struct InterfaceInfo {
    pub ifname: String,
    pub flags: u32,
    pub mac: Option<String>,
    pub mtu: Option<u32>,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub carrier: bool,
}

impl InterfaceInfo {
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.flags & libc::IFF_UP as u32 != 0
    }

    #[must_use]
    pub fn is_loopback(&self) -> bool {
        self.flags & libc::IFF_LOOPBACK as u32 != 0
    }

    #[must_use]
    pub fn kind(&self) -> DeviceKind {
        if self.is_loopback() {
            DeviceKind::Loopback
        } else if self.ifname.starts_with("wlan") {
            DeviceKind::Wifi
        } else if self.ifname.is_empty() {
            DeviceKind::Other
        } else {
            DeviceKind::Ethernet
        }
    }
}

/// Interfaces that are kernel plumbing rather than user-facing links.
#[must_use]
pub fn is_virtual(ifname: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "lo",
        "pflog",
        "pfsync",
        "enc",
        "gif",
        "stf",
        "bridge",
        "lagg",
        "vlan",
        "tun",
        "tap",
        "ipfw",
        "epair",
        "vmnet",
        "wg",
        "tailscale",
        "docker",
        "vxlan",
    ];
    PREFIXES.iter().any(|prefix| ifname.starts_with(prefix))
}

/// Enumerate interfaces, merging AF_LINK, AF_INET and AF_INET6 records.
pub fn list_interfaces() -> Result<Vec<InterfaceInfo>> {
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs writes a valid linked list or returns an error.
    let rc = unsafe { libc::getifaddrs(&mut head) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error()).context("getifaddrs 失败");
    }
    let _guard = IfAddrs(head);
    let mut interfaces: BTreeMap<String, InterfaceInfo> = BTreeMap::new();

    let mut cursor = head;
    while !cursor.is_null() {
        // SAFETY: cursor is a node of the list returned by getifaddrs.
        let item = unsafe { &*cursor };
        if !item.ifa_name.is_null() {
            // SAFETY: ifa_name is NUL-terminated by getifaddrs.
            let name = unsafe { CStr::from_ptr(item.ifa_name) }
                .to_string_lossy()
                .into_owned();
            let entry = interfaces
                .entry(name.clone())
                .or_insert_with(|| InterfaceInfo {
                    ifname: name,
                    ..InterfaceInfo::default()
                });
            if entry.flags == 0 {
                entry.flags = item.ifa_flags;
            }
            if !item.ifa_addr.is_null() {
                // SAFETY: ifa_addr points to a sockaddr whose first byte is the family on FreeBSD.
                let family = unsafe { (*item.ifa_addr).sa_family as i32 };
                match family {
                    libc::AF_LINK => {
                        entry.carrier = item.ifa_flags & libc::IFF_RUNNING as u32 != 0;
                        // SAFETY: AF_LINK records carry a sockaddr_dl.
                        let dl = unsafe { &*item.ifa_addr.cast::<libc::sockaddr_dl>() };
                        if entry.mac.is_none() {
                            entry.mac = link_address(dl);
                        }
                        if !item.ifa_data.is_null() {
                            // SAFETY: AF_LINK records carry an if_data in ifa_data.
                            let data = unsafe { &*item.ifa_data.cast::<libc::if_data>() };
                            if data.ifi_mtu > 0 {
                                entry.mtu = Some(data.ifi_mtu);
                            }
                        }
                    }
                    libc::AF_INET => {
                        let address = ipv4_record(item.ifa_addr, item.ifa_netmask);
                        if let Some(address) = address
                            && !entry.ipv4.contains(&address)
                        {
                            entry.ipv4.push(address);
                        }
                    }
                    libc::AF_INET6 => {
                        let address = ipv6_record(item.ifa_addr, item.ifa_netmask);
                        if let Some(address) = address
                            && !entry.ipv6.contains(&address)
                        {
                            entry.ipv6.push(address);
                        }
                    }
                    _ => {}
                }
            }
        }
        cursor = item.ifa_next;
    }

    Ok(interfaces.into_values().collect())
}

struct IfAddrs(*mut libc::ifaddrs);

impl Drop for IfAddrs {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: list originated from getifaddrs and is freed exactly once.
            unsafe { libc::freeifaddrs(self.0) };
        }
    }
}

fn link_address(dl: &libc::sockaddr_dl) -> Option<String> {
    let nlen = dl.sdl_nlen as usize;
    let alen = dl.sdl_alen as usize;
    if alen == 0 || nlen + alen > dl.sdl_data.len() {
        return None;
    }
    let bytes: Vec<u8> = dl.sdl_data[nlen..nlen + alen]
        .iter()
        .map(|byte| *byte as u8)
        .collect();
    if bytes.iter().all(|byte| *byte == 0) {
        return None;
    }
    Some(format_mac(&bytes))
}

fn ipv4_record(address: *mut libc::sockaddr, netmask: *mut libc::sockaddr) -> Option<String> {
    // SAFETY: AF_INET records carry sockaddr_in.
    let sin = unsafe { &*address.cast::<libc::sockaddr_in>() };
    let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
    let prefix = if netmask.is_null() {
        32
    } else {
        // SAFETY: non-null netmask for AF_INET is a sockaddr_in.
        let mask = unsafe { &*netmask.cast::<libc::sockaddr_in>() };
        u32::from_be(mask.sin_addr.s_addr).count_ones()
    };
    Some(format!("{ip}/{prefix}"))
}

fn ipv6_record(address: *mut libc::sockaddr, netmask: *mut libc::sockaddr) -> Option<String> {
    // SAFETY: AF_INET6 records carry sockaddr_in6.
    let sin6 = unsafe { &*address.cast::<libc::sockaddr_in6>() };
    let ip = Ipv6Addr::from(sin6.sin6_addr.s6_addr);
    let prefix = if netmask.is_null() {
        128
    } else {
        // SAFETY: non-null netmask for AF_INET6 is a sockaddr_in6.
        let mask = unsafe { &*netmask.cast::<libc::sockaddr_in6>() };
        u64::from_be_bytes(mask.sin6_addr.s6_addr[..8].try_into().ok()?).count_ones()
            + u64::from_be_bytes(mask.sin6_addr.s6_addr[8..].try_into().ok()?).count_ones()
    };
    Some(format!("{ip}/{prefix}"))
}

#[must_use]
pub fn format_mac(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            out.push(':');
        }
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Build the user-facing shell of a device. The manager applies policy and state.
#[must_use]
pub fn to_device(info: &InterfaceInfo, state: DeviceState) -> NetworkDevice {
    NetworkDevice {
        ifname: info.ifname.clone(),
        kind: info.kind(),
        state,
        managed: !info.is_loopback() && !is_virtual(&info.ifname),
        enabled: info.enabled(),
        carrier: info.carrier,
        mac: info.mac.clone(),
        mtu: info.mtu,
        ipv4: info.ipv4.clone(),
        ipv6: info.ipv6.clone(),
        ssid: None,
        networks: Vec::new(),
        last_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_mac() {
        assert_eq!(format_mac(&[0x00, 0x11, 0x22, 0xab]), "00:11:22:ab");
        assert_eq!(format_mac(&[]), "");
    }

    #[test]
    fn classifies_interfaces() {
        let mut info = InterfaceInfo {
            ifname: "wlan0".to_owned(),
            ..InterfaceInfo::default()
        };
        assert_eq!(info.kind(), DeviceKind::Wifi);
        info.ifname = "ue0".to_owned();
        assert_eq!(info.kind(), DeviceKind::Ethernet);
        info.flags = libc::IFF_LOOPBACK as u32;
        assert_eq!(info.kind(), DeviceKind::Loopback);
        assert!(is_virtual("lo0"));
        assert!(is_virtual("bridge0"));
        assert!(!is_virtual("ue0"));
    }

    #[test]
    fn lists_real_interfaces() {
        let interfaces = list_interfaces().unwrap();
        assert!(!interfaces.is_empty());
        assert!(interfaces.iter().any(|info| !info.ifname.is_empty()));
    }
}
