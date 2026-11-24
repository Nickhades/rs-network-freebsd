//! Runtime discovery for FreeBSD base/ports tools and directories.
//!
//! Distribution images differ in whether wpa_supplicant/dhclient live in
//! /sbin, /usr/sbin, or /usr/local/sbin. Explicit flags/environment always win;
//! otherwise we search PATH and a small set of conventional fallbacks.

use std::env;
use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Find an executable by explicit environment override, PATH, then candidates.
#[must_use]
pub fn find_executable(name: &str, env_var: &str, candidates: &[&str]) -> Option<PathBuf> {
    if let Some(value) = env::var_os(env_var)
        && !value.is_empty()
    {
        return Some(PathBuf::from(value));
    }
    if let Some(paths) = env::var_os("PATH") {
        for directory in env::split_paths(&paths) {
            let candidate = directory.join(name);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    candidates
        .iter()
        .map(Path::new)
        .find(|candidate| is_executable(candidate))
        .map(Path::to_path_buf)
}

#[must_use]
pub fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[must_use]
pub fn ifconfig() -> Option<PathBuf> {
    find_executable(
        "ifconfig",
        "IFCONFIG",
        &[
            "/sbin/ifconfig",
            "/usr/sbin/ifconfig",
            "/usr/local/sbin/ifconfig",
        ],
    )
}

#[must_use]
pub fn dhclient() -> Option<PathBuf> {
    find_executable(
        "dhclient",
        "DHCLIENT",
        &[
            "/sbin/dhclient",
            "/usr/sbin/dhclient",
            "/usr/local/sbin/dhclient",
        ],
    )
}

#[must_use]
pub fn dhcpcd() -> Option<PathBuf> {
    find_executable(
        "dhcpcd",
        "DHCPCD",
        &[
            "/usr/local/sbin/dhcpcd",
            "/usr/sbin/dhcpcd",
            "/sbin/dhcpcd",
            "/usr/local/bin/dhcpcd",
        ],
    )
}

pub fn wpa_cli() -> Option<PathBuf> {
    find_executable(
        "wpa_cli",
        "WPA_CLI",
        &[
            "/usr/sbin/wpa_cli",
            "/usr/local/sbin/wpa_cli",
            "/sbin/wpa_cli",
            "/usr/bin/wpa_cli",
            "/usr/local/bin/wpa_cli",
        ],
    )
}

#[must_use]
pub fn sysctl() -> Option<PathBuf> {
    find_executable(
        "sysctl",
        "SYSCTL",
        &["/sbin/sysctl", "/usr/sbin/sysctl", "/usr/bin/sysctl"],
    )
}

#[must_use]
pub fn pciconf() -> Option<PathBuf> {
    find_executable(
        "pciconf",
        "PCICONF",
        &[
            "/usr/sbin/pciconf",
            "/sbin/pciconf",
            "/usr/local/sbin/pciconf",
        ],
    )
}

/// Find the wpa_supplicant control directory.
///
/// An explicit path (flag or WPA_CTRL_DIR) wins. Otherwise look for a
/// conventional directory containing at least one control socket. Returning
/// None is valid and means "let wpa_cli use its compiled-in default".
#[must_use]
pub fn find_wpa_ctrl_dir(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path);
    }
    if let Some(value) = env::var_os("WPA_CTRL_DIR")
        && !value.is_empty()
    {
        return Some(PathBuf::from(value));
    }

    let direct = PathBuf::from("/var/run/wpa_supplicant");
    if contains_control_socket(&direct) {
        return Some(direct);
    }

    if let Ok(entries) = fs::read_dir("/var/run") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("wpa_supplicant") {
                continue;
            }
            let path = entry.path();
            if path.is_dir() && contains_control_socket(&path) {
                return Some(path);
            }
        }
    }
    None
}

fn contains_control_socket(directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    entries
        .flatten()
        .any(|entry| entry.file_type().is_ok_and(|kind| kind.is_socket()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_check_rejects_directories() {
        assert!(!is_executable(Path::new("/")));
        assert!(is_executable(Path::new("/bin/sh")) || is_executable(Path::new("/usr/bin/sh")));
    }

    #[test]
    fn explicit_control_dir_wins() {
        let explicit = PathBuf::from("/tmp/does-not-need-to-exist");
        assert_eq!(find_wpa_ctrl_dir(Some(explicit.clone())), Some(explicit));
    }
}
