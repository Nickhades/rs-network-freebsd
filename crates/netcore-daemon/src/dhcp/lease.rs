//! Lossless-enough lease parsing for the two supported DHCP clients.
//!
//! dhclient and dhcpcd store leases in different formats.  We only need the
//! IPv4 address, prefix, router and DNS servers for the event stream; the
//! kernel remains the authority for the actual address on the interface.

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

use crate::system::{self, has_ipv4};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeaseDetails {
    pub(crate) ip: IpAddr,
    pub(crate) subnet: u8,
    pub(crate) gateway: IpAddr,
    pub(crate) dns: Vec<IpAddr>,
}

/// Read the address currently present on the interface and, when readable,
/// enrich it with options from the client lease file.
pub(crate) fn lease_for_interface(
    ifname: &str,
    lease_paths: &[std::path::PathBuf],
) -> Option<LeaseDetails> {
    let info = system::interfaces()
        .ok()?
        .into_iter()
        .find(|item| item.ifname == ifname)?;
    if !has_ipv4(&info) {
        return None;
    }
    let (ip, prefix) = parse_cidr(info.ipv4.first()?)?;
    let mut details = lease_paths
        .iter()
        .find_map(|path| parse_lease_file(path))
        .unwrap_or(LeaseDetails {
            ip,
            subnet: prefix,
            gateway: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            dns: Vec::new(),
        });
    details.ip = ip;
    if !(1..=32).contains(&details.subnet) {
        details.subnet = prefix;
    }
    Some(details)
}

pub(crate) fn parse_lease_file(path: &Path) -> Option<LeaseDetails> {
    let text = std::fs::read_to_string(path).ok()?;
    parse_lease_text(&text)
}

/// Parse the last lease block in a dhclient file or the key/value lines used
/// by dhcpcd.  Unknown lines are ignored; malformed values never panic.
#[must_use]
pub(crate) fn parse_lease_text(text: &str) -> Option<LeaseDetails> {
    let block = text
        .rsplit("lease {")
        .next()
        .filter(|block| block.len() != text.len())
        .unwrap_or(text);
    let mut ip: Option<IpAddr> = None;
    let mut subnet: Option<u8> = None;
    let mut gateway: Option<IpAddr> = None;
    let mut dns = Vec::new();
    for raw in block.lines() {
        let line = raw.trim().trim_end_matches(';').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(' ').or_else(|| line.split_once('=')) else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        let value = value.trim_matches('"');
        match key {
            "fixed-address" | "ip_address" | "ip-address" => {
                if ip.is_none() {
                    ip = parse_ip(value);
                }
            }
            "option" => {
                let mut parts = value.splitn(2, ' ');
                let Some(option) = parts.next() else { continue };
                let Some(value) = parts.next() else { continue };
                match option {
                    "subnet-mask" | "subnet_mask" => {
                        subnet = parse_mask(value);
                    }
                    "routers" | "router" => {
                        gateway = first_ip(value);
                    }
                    "domain-name-servers" | "domain_name_servers" => {
                        dns = parse_ips(value);
                    }
                    _ => {}
                }
            }
            "subnet_mask" | "subnet-mask" => {
                subnet = parse_mask(value);
            }
            "routers" | "router" => {
                gateway = first_ip(value);
            }
            "domain_name_servers" | "domain-name-servers" | "dns" => {
                dns = parse_ips(value);
            }
            _ => {}
        }
    }
    let ip = ip?;
    Some(LeaseDetails {
        ip,
        subnet: subnet.unwrap_or(24),
        gateway: gateway.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        dns,
    })
}

fn parse_cidr(value: &str) -> Option<(IpAddr, u8)> {
    let (ip, prefix) = value.split_once('/')?;
    let ip = parse_ip(ip)?;
    let prefix = prefix.parse::<u8>().ok()?;
    Some((ip, prefix.min(32)))
}

fn parse_ip(value: &str) -> Option<IpAddr> {
    value.trim().trim_matches('"').parse().ok()
}

fn first_ip(value: &str) -> Option<IpAddr> {
    parse_ips(value).into_iter().next()
}

fn parse_ips(value: &str) -> Vec<IpAddr> {
    value
        .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .filter_map(parse_ip)
        .collect()
}

fn parse_mask(value: &str) -> Option<u8> {
    let value = value.trim();
    if let Ok(prefix) = value.parse::<u8>() {
        return (prefix <= 32).then_some(prefix);
    }
    let mask: Ipv4Addr = value.parse().ok()?;
    Some(u32::from(mask).count_ones() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dhclient_lease() {
        let text = r#"lease {
  interface "wlan0";
  fixed-address 192.0.2.44;
  option subnet-mask 255.255.255.0;
  option routers 192.0.2.1;
  option domain-name-servers 192.0.2.53, 192.0.2.54;
}"#;
        let lease = parse_lease_text(text).expect("lease");
        assert_eq!(lease.ip, "192.0.2.44".parse::<IpAddr>().unwrap());
        assert_eq!(lease.subnet, 24);
        assert_eq!(lease.gateway, "192.0.2.1".parse::<IpAddr>().unwrap());
        assert_eq!(lease.dns.len(), 2);
    }

    #[test]
    fn parses_dhcpcd_key_values() {
        let text = "ip_address=198.51.100.7\nsubnet_mask=255.255.255.128\nrouters=198.51.100.1\ndomain_name_servers=198.51.100.53\n";
        let lease = parse_lease_text(text).expect("lease");
        assert_eq!(lease.subnet, 25);
        assert_eq!(lease.gateway, "198.51.100.1".parse::<IpAddr>().unwrap());
        assert_eq!(lease.dns, vec!["198.51.100.53".parse::<IpAddr>().unwrap()]);
    }

    #[test]
    fn rejects_missing_address() {
        assert!(parse_lease_text("lease { option routers 192.0.2.1; }").is_none());
    }
}
