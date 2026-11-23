//! FreeBSD NETLINK_ROUTE monitor.
//!
//! FreeBSD 13.2+ implements rtnetlink. We only need multicast notifications:
//! link up/down and IPv4/IPv6 address changes. The parser is intentionally
//! tiny and defensive; a malformed datagram is dropped, never trusted.
//!
//! When --capsicum is set, the socket and parser live in a child process that
//! calls cap_enter(2). The privileged daemon only consumes newline-delimited
//! event tags from the helper's stdout.

use std::io::Write as _;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

const AF_NETLINK: libc::c_int = 38;
const NETLINK_ROUTE: libc::c_int = 0;
const NLMSG_HDRLEN: usize = 16;
const RTM_NEWLINK: u16 = 16;
const RTM_DELLINK: u16 = 17;
const RTM_NEWADDR: u16 = 20;
const RTM_DELADDR: u16 = 21;
const RTMGRP_LINK: u32 = 0x1;
const RTMGRP_IPV4_IFADDR: u32 = 0x10;
const RTMGRP_IPV6_IFADDR: u32 = 0x100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetlinkEvent {
    Link,
    Address,
}

impl NetlinkEvent {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Address => "addr",
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NlMsgHdr {
    nlmsg_len: u32,
    nlmsg_type: u16,
    nlmsg_flags: u16,
    nlmsg_seq: u32,
    nlmsg_pid: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SockaddrNl {
    nl_len: u8,
    nl_family: u8,
    nl_pad: u16,
    nl_pid: u32,
    nl_groups: u32,
}

/// Open and bind the route notification socket.
fn open_socket() -> Result<OwnedFd> {
    // SAFETY: plain socket(2) call with constants from FreeBSD's netlink ABI.
    let fd = unsafe { libc::socket(AF_NETLINK, libc::SOCK_RAW, NETLINK_ROUTE) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error()).context("创建 NETLINK_ROUTE 套接字失败");
    }
    // SAFETY: fd is newly owned and not used elsewhere on failure paths.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    let address = SockaddrNl {
        nl_len: std::mem::size_of::<SockaddrNl>() as u8,
        nl_family: AF_NETLINK as u8,
        nl_pad: 0,
        nl_pid: 0,
        nl_groups: RTMGRP_LINK | RTMGRP_IPV4_IFADDR | RTMGRP_IPV6_IFADDR,
    };
    // SAFETY: address is a properly initialized sockaddr_nl of the declared size.
    let rc = unsafe {
        libc::bind(
            owned.as_raw_fd(),
            (&raw const address).cast::<libc::sockaddr>(),
            std::mem::size_of::<SockaddrNl>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error()).context("绑定 NETLINK_ROUTE 多播组失败");
    }
    Ok(owned)
}

fn set_nonblocking(fd: &OwnedFd) -> Result<()> {
    // SAFETY: fcntl operates on a live descriptor.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error()).context("fcntl(F_GETFL) 失败");
    }
    // SAFETY: same live descriptor, valid flag value.
    let rc = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error()).context("fcntl(F_SETFL) 失败");
    }
    Ok(())
}

fn parse_events(buffer: &[u8]) -> Vec<NetlinkEvent> {
    let mut events = Vec::new();
    let mut offset = 0_usize;
    while offset + NLMSG_HDRLEN <= buffer.len() {
        // SAFETY: the 16-byte header is within the slice and NlMsgHdr is plain old data.
        let header =
            unsafe { std::ptr::read_unaligned(buffer[offset..].as_ptr().cast::<NlMsgHdr>()) };
        let length = header.nlmsg_len as usize;
        if length < NLMSG_HDRLEN || offset + length > buffer.len() {
            break;
        }
        let event = match header.nlmsg_type {
            RTM_NEWLINK | RTM_DELLINK => Some(NetlinkEvent::Link),
            RTM_NEWADDR | RTM_DELADDR => Some(NetlinkEvent::Address),
            _ => None,
        };
        if let Some(event) = event
            && events.last().copied() != Some(event)
        {
            events.push(event);
        }
        offset += (length + 3) & !3;
    }
    events
}

fn recv_events(fd: libc::c_int, buffer: &mut [u8]) -> std::io::Result<Vec<NetlinkEvent>> {
    // SAFETY: buffer is valid for writes and length is exact.
    let count = unsafe { libc::recv(fd, buffer.as_mut_ptr().cast(), buffer.len(), 0) };
    if count < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(parse_events(&buffer[..count as usize]))
}

/// Spawn the in-process async listener and forward debounced events.
pub async fn spawn_in_process(
    tx: mpsc::Sender<NetlinkEvent>,
    active: Arc<AtomicBool>,
) -> Result<()> {
    let fd = open_socket()?;
    set_nonblocking(&fd)?;
    // SAFETY: the AsyncFd owns the descriptor for its entire lifetime and the
    // OwnedFd is not duplicated or closed elsewhere.
    let async_fd = unsafe { tokio::io::unix::AsyncFd::register(fd) }
        .map_err(|error| anyhow::anyhow!("注册 Netlink 套接字失败: {error}"))?;
    active.store(true, Ordering::SeqCst);
    tracing::info!("Netlink 监听已启动 (NETLINK_ROUTE)");
    tokio::spawn(async move {
        let mut buffer = vec![0_u8; 65_536];
        loop {
            let mut guard = match async_fd.readable().await {
                Ok(guard) => guard,
                Err(error) => {
                    tracing::error!(%error, "Netlink readable() 失败");
                    break;
                }
            };
            match guard.try_io(|inner| recv_events(inner.as_raw_fd(), &mut buffer)) {
                Ok(Ok(events)) => {
                    for event in events {
                        if tx.send(event).await.is_err() {
                            return;
                        }
                    }
                }
                Ok(Err(error)) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                Ok(Err(error)) => {
                    tracing::warn!(%error, "Netlink 接收失败，改为轮询");
                    break;
                }
                Err(_would_block) => continue,
            }
        }
        active.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// Spawn the capsicum helper and consume its event stream.
pub async fn spawn_capsicum_helper(
    tx: mpsc::Sender<NetlinkEvent>,
    active: Arc<AtomicBool>,
) -> Result<()> {
    let exe = std::env::current_exe().context("无法定位 netcore-daemon 可执行文件")?;
    let mut child = tokio::process::Command::new(exe)
        .arg("--netlink-helper")
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context("启动 Capsicum Netlink helper 失败")?;
    let stdout = child.stdout.take().context("helper 缺少 stdout")?;
    tracing::info!(pid = child.id(), "Capsicum Netlink helper 已启动");
    tokio::spawn(async move {
        let _child = child;
        let mut lines = BufReader::new(stdout).lines();
        let mut ready = false;
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => match line.trim() {
                    "ready" => {
                        ready = true;
                        active.store(true, Ordering::SeqCst);
                        tracing::info!("Capsicum helper 进入能力模式，Netlink 可用");
                    }
                    "link" => {
                        if tx.send(NetlinkEvent::Link).await.is_err() {
                            break;
                        }
                    }
                    "addr" => {
                        if tx.send(NetlinkEvent::Address).await.is_err() {
                            break;
                        }
                    }
                    other => tracing::debug!(line = other, "忽略 helper 输出"),
                },
                Ok(None) => break,
                Err(error) => {
                    tracing::warn!(%error, "读取 Capsicum helper 输出失败");
                    break;
                }
            }
        }
        active.store(false, Ordering::SeqCst);
        tracing::warn!(ready, "Capsicum Netlink helper 已退出，改用轮询");
    });
    Ok(())
}

/// Entry point for the hidden --netlink-helper process.
pub fn capsicum_helper_main() -> Result<()> {
    let fd = open_socket()?;
    enter_capability_mode()?;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "ready")?;
    stdout.flush()?;
    let mut buffer = vec![0_u8; 65_536];
    loop {
        // SAFETY: fd is a live socket; buffer is valid for the stated length.
        let count =
            unsafe { libc::recv(fd.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len(), 0) };
        if count < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            bail!("helper recv 失败: {error}");
        }
        for event in parse_events(&buffer[..count as usize]) {
            writeln!(stdout, "{}", event.as_str())?;
            stdout.flush()?;
        }
    }
}

#[cfg(target_os = "freebsd")]
fn enter_capability_mode() -> Result<()> {
    unsafe extern "C" {
        fn cap_enter() -> libc::c_int;
    }
    // SAFETY: cap_enter takes no arguments and returns 0/-1 with errno.
    let rc = unsafe { cap_enter() };
    if rc != 0 {
        return Err(std::io::Error::last_os_error()).context("cap_enter 失败");
    }
    Ok(())
}

#[cfg(not(target_os = "freebsd"))]
fn enter_capability_mode() -> Result<()> {
    bail!("Capsicum 仅支持 FreeBSD")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(message_type: u16, length: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&length.to_ne_bytes());
        bytes.extend_from_slice(&message_type.to_ne_bytes());
        bytes.extend_from_slice(&0_u16.to_ne_bytes());
        bytes.extend_from_slice(&0_u32.to_ne_bytes());
        bytes.extend_from_slice(&0_u32.to_ne_bytes());
        bytes
    }

    #[test]
    fn parses_link_and_address_notifications() {
        let mut link = header(RTM_NEWLINK, 20);
        link.extend_from_slice(&[0; 4]);
        let mut addr = header(RTM_NEWADDR, 20);
        addr.extend_from_slice(&[0; 4]);
        let mut buffer = link.clone();
        buffer.extend_from_slice(&addr);
        assert_eq!(
            parse_events(&buffer),
            vec![NetlinkEvent::Link, NetlinkEvent::Address]
        );
    }

    #[test]
    fn ignores_truncated_messages() {
        let mut buffer = header(RTM_NEWLINK, 64);
        buffer.truncate(10);
        assert!(parse_events(&buffer).is_empty());
    }

    #[test]
    fn ignores_unrelated_messages() {
        let buffer = header(0x99, 16);
        assert!(parse_events(&buffer).is_empty());
    }

    #[cfg(target_os = "freebsd")]
    #[test]
    fn socket_can_be_opened_and_bound() {
        let fd = open_socket().unwrap();
        assert!(fd.as_raw_fd() >= 0);
    }
}
