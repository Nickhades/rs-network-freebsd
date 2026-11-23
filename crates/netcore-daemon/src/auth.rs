//! Authorization for privileged D-Bus methods.
//!
//! The daemon never trusts the bus name alone for mutations. On the system bus
//! it resolves the caller's UID through org.freedesktop.DBus, allows root, and
//! either checks the configured administrator group or asks polkit for
//! org.netcore.manage (with interactive authentication allowed). Read-only
//! methods bypass this check.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::sync::Mutex;

use netcore_common::POLKIT_ACTION;
use zbus::message::Header;
use zbus::names::BusName;
use zbus::{Connection, Proxy, Variant};

use crate::config::{AuthChoice, BusChoice};
use crate::error::ManagerError;

static GROUP_DB_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone)]
pub struct Authorizer {
    mode: AuthChoice,
    bus: BusChoice,
    allow_group: String,
}

impl Authorizer {
    #[must_use]
    pub fn new(mode: AuthChoice, bus: BusChoice, allow_group: String) -> Self {
        Self {
            mode,
            bus,
            allow_group,
        }
    }

    pub async fn authorize(
        &self,
        connection: &Connection,
        header: &Header<'_>,
    ) -> Result<(), ManagerError> {
        if matches!(self.mode, AuthChoice::None) {
            tracing::warn!("授权检查已禁用，仅应在隔离的开发会话中使用");
            return Ok(());
        }
        if self.bus == BusChoice::Session {
            return Ok(());
        }
        let Some(sender) = header.sender() else {
            return Err(ManagerError::unauthorized("D-Bus 调用缺少发送者"));
        };
        let sender_name = sender.to_string();
        let dbus = zbus::fdo::DBusProxy::new(connection)
            .await
            .map_err(|error| ManagerError::operation(format!("查询 D-Bus 凭据失败: {error}")))?;
        let bus_name = BusName::try_from(sender_name.clone())
            .map_err(|error| ManagerError::operation(format!("无效的 D-Bus 名称: {error}")))?;
        let uid = dbus
            .get_connection_unix_user(bus_name)
            .await
            .map_err(|error| ManagerError::operation(format!("查询调用者 UID 失败: {error}")))?;
        if uid == 0 {
            return Ok(());
        }

        let group_mode = matches!(self.mode, AuthChoice::Auto | AuthChoice::Group);
        if group_mode && user_in_group(uid, &self.allow_group) {
            tracing::debug!(uid, group = %self.allow_group, "按组成员授权");
            return Ok(());
        }

        let polkit_mode = matches!(self.mode, AuthChoice::Auto | AuthChoice::Polkit);
        if polkit_mode {
            match check_polkit(connection, &sender_name, uid).await {
                Ok(true) => {
                    tracing::info!(uid, "polkit 已授权网络管理操作");
                    return Ok(());
                }
                Ok(false) => {
                    tracing::warn!(uid, "polkit 拒绝了网络管理操作");
                }
                Err(error) => {
                    tracing::warn!(uid, %error, "polkit 授权检查失败");
                    if self.mode == AuthChoice::Polkit {
                        return Err(ManagerError::unauthorized(format!(
                            "polkit 授权失败: {error}"
                        )));
                    }
                }
            }
        }

        Err(ManagerError::unauthorized(format!(
            "用户 UID {uid} 无权修改网络配置；请加入 {} 组或完成 polkit 认证",
            self.allow_group
        )))
    }
}

async fn check_polkit(connection: &Connection, sender: &str, uid: u32) -> anyhow::Result<bool> {
    let proxy = Proxy::new_owned(
        connection.clone(),
        "org.freedesktop.PolicyKit1",
        "/org/freedesktop/PolicyKit1/Authority",
        "org.freedesktop.PolicyKit1.Authority",
    )
    .await?;
    let mut subject_details: HashMap<String, Variant<'static>> = HashMap::new();
    subject_details.insert("name".to_owned(), Variant::from(sender.to_owned()));
    let subject = ("system-bus-name".to_owned(), subject_details);
    let details: HashMap<String, String> = HashMap::new();
    // 1 == AllowUserInteraction.
    let (authorized, _challenge, _result_details): (bool, bool, HashMap<String, String>) = proxy
        .call(
            "CheckAuthorization",
            &(subject, POLKIT_ACTION, details, 1_u32, String::new()),
        )
        .await?;
    tracing::debug!(uid, authorized, "polkit CheckAuthorization 返回");
    Ok(authorized)
}

fn user_in_group(uid: u32, group_name: &str) -> bool {
    if group_name.is_empty() {
        return false;
    }
    let Ok(group_name) = CString::new(group_name) else {
        return false;
    };
    let (Some(username), Some(primary_gid), Some(target_gid)) = (
        lookup_username(uid),
        lookup_primary_gid(uid),
        lookup_group_gid(&group_name),
    ) else {
        return false;
    };
    if primary_gid == target_gid {
        return true;
    }
    let Ok(username) = CString::new(username) else {
        return false;
    };
    let mut groups = vec![0 as libc::gid_t; 32];
    let mut count = groups.len() as libc::c_int;
    // SAFETY: pointers refer to valid buffers and count is the element capacity.
    let rc = unsafe {
        libc::getgrouplist(
            username.as_ptr(),
            primary_gid,
            groups.as_mut_ptr(),
            &mut count,
        )
    };
    if rc < 0 {
        groups.resize(count.max(0) as usize, 0);
        let mut retry_count = groups.len() as libc::c_int;
        // SAFETY: resized buffer now has the required capacity.
        let retry = unsafe {
            libc::getgrouplist(
                username.as_ptr(),
                primary_gid,
                groups.as_mut_ptr(),
                &mut retry_count,
            )
        };
        if retry < 0 {
            return false;
        }
        count = retry_count;
    }
    groups[..count.max(0) as usize].contains(&target_gid)
}

fn lookup_username(uid: u32) -> Option<String> {
    let size = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let size = if size <= 0 { 4096 } else { size as usize };
    let mut buffer = vec![0_u8; size];
    // SAFETY: passwd is a plain-old-data struct; zero is a valid initial value.
    let mut passwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: buffers are valid for the call and uid is just a number.
    let rc = unsafe {
        libc::getpwuid_r(
            uid,
            &mut passwd,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() || passwd.pw_name.is_null() {
        return None;
    }
    // SAFETY: pw_name points into buffer and is NUL-terminated.
    Some(
        unsafe { CStr::from_ptr(passwd.pw_name) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn lookup_primary_gid(uid: u32) -> Option<libc::gid_t> {
    let size = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let size = if size <= 0 { 4096 } else { size as usize };
    let mut buffer = vec![0_u8; size];
    // SAFETY: zeroed passwd is valid storage for getpwuid_r.
    let mut passwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: valid buffers and uid.
    let rc = unsafe {
        libc::getpwuid_r(
            uid,
            &mut passwd,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    (rc == 0 && !result.is_null()).then_some(passwd.pw_gid)
}

fn lookup_group_gid(name: &CStr) -> Option<libc::gid_t> {
    let _guard = GROUP_DB_LOCK.lock().expect("group mutex poisoned");
    // SAFETY: getgrnam is not thread-safe; the mutex serializes access.
    let group = unsafe { libc::getgrnam(name.as_ptr()) };
    if group.is_null() {
        None
    } else {
        // SAFETY: non-null pointer returned by getgrnam.
        Some(unsafe { (*group).gr_gid })
    }
}
