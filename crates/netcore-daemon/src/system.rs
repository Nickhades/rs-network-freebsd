//! Small, explicit wrappers around FreeBSD userland tools and syscalls.

use std::ffi::CStr;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use tokio::process::Command;

use crate::iface::{InterfaceInfo, list_interfaces};

/// Validate an interface name before passing it to a privileged command.
pub fn validate_ifname(ifname: &str) -> Result<()> {
    if ifname.is_empty() || ifname.len() >= libc::IFNAMSIZ {
        bail!("接口名长度无效");
    }
    if !ifname
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'))
    {
        bail!("接口名包含非法字符: {ifname}");
    }
    Ok(())
}

/// Run a command with a hard timeout and turn a non-zero exit into an error.
pub async fn run_checked(program: &Path, args: &[&str]) -> Result<String> {
    tracing::debug!(program = %program.display(), ?args, "执行系统命令");
    let output = tokio::time::timeout(
        Duration::from_secs(20),
        Command::new(program).args(args).env("LC_ALL", "C").output(),
    )
    .await
    .with_context(|| format!("{} 执行超时", program.display()))?
    .with_context(|| format!("无法执行 {}", program.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if !output.status.success() {
        let detail = if stderr.is_empty() { stdout } else { stderr };
        bail!(
            "{} 失败 ({}): {}",
            program.display(),
            output.status,
            detail.chars().take(500).collect::<String>()
        );
    }
    Ok(stdout)
}

pub async fn set_link_up(ifconfig: &Path, ifname: &str, up: bool) -> Result<()> {
    validate_ifname(ifname)?;
    run_checked(ifconfig, &[ifname, if up { "up" } else { "down" }]).await?;
    Ok(())
}

#[must_use]
pub fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

#[must_use]
pub fn current_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[must_use]
pub fn hostname() -> String {
    let mut buffer = [0_u8; 256];
    // SAFETY: buffer is writable and its length is passed correctly.
    let rc = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if rc != 0 {
        return "freebsd".to_owned();
    }
    // SAFETY: gethostname NUL-terminates on success; keep a bounded fallback.
    let cstr = unsafe { CStr::from_ptr(buffer.as_ptr().cast()) };
    cstr.to_string_lossy().into_owned()
}

pub fn interfaces() -> Result<Vec<InterfaceInfo>> {
    list_interfaces()
}

#[must_use]
pub fn has_ipv4(info: &InterfaceInfo) -> bool {
    !info.ipv4.is_empty()
}

#[must_use]
pub fn privilege_hint() -> &'static str {
    "需要 root 权限；请通过 rc.d 或 sudo 运行 netcore-daemon"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_interface_names() {
        assert!(validate_ifname("wlan0").is_ok());
        assert!(validate_ifname("ue0").is_ok());
        assert!(validate_ifname("").is_err());
        assert!(validate_ifname("wlan0;reboot").is_err());
        assert!(validate_ifname("wlan 0").is_err());
    }

    #[test]
    fn root_check_does_not_crash() {
        let _ = is_root();
    }
}
