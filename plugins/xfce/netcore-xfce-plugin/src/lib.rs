//! Native XFCE Panel 4.20 network plugin for netcore.
//!
//! The plugin is a resident shared object loaded into xfce4-panel. It talks to
//! the daemon over GDBus asynchronously, never blocks the panel main loop on
//! network operations, and rebuilds its menu only when it is about to open.

use std::cell::RefCell;
use std::rc::Rc;

use gio::prelude::*;
use netcore_common::{
    DBUS_INTERFACE, DBUS_PATH, DBUS_SERVICE, DeviceKind, DeviceState, ManagerSnapshot,
    ManagerState, NetworkDevice,
};
use xfce_panel::glib;
use xfce_panel::gtk::prelude::*;
use xfce_panel::{PanelPlugin, Plugin, gtk};

/// Per-plugin runtime state.
#[derive(Default)]
pub struct Runtime {
    connection: Option<gio::DBusConnection>,
    root: Option<gtk::EventBox>,
    icon: Option<gtk::Image>,
    label: Option<gtk::Label>,
    menu: Option<gtk::Menu>,
    snapshot: Option<ManagerSnapshot>,
    available: bool,
    last_error: Option<String>,
    generation: u64,
    subscription: Option<gio::SignalSubscription>,
}

type Shared = Rc<RefCell<Runtime>>;

#[derive(Default)]
pub struct NetcorePlugin;

impl PanelPlugin for NetcorePlugin {
    const TYPE_NAME: &'static str = "XfceNetcorePlugin";
    type State = Rc<RefCell<Runtime>>;

    fn construct(state: &mut Self::State, plugin: &Plugin) {
        let runtime: Shared = Rc::new(RefCell::new(Runtime::default()));
        let root = gtk::EventBox::new();
        root.set_tooltip_text(Some("netcore 网络管理器"));
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        content.set_border_width(2);
        let icon =
            gtk::Image::from_icon_name(Some("network-offline-symbolic"), gtk::IconSize::Menu);
        let label = gtk::Label::new(Some("离线"));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(14);
        content.pack_start(&icon, false, false, 0);
        content.pack_start(&label, false, false, 0);
        root.add(&content);
        plugin.add_widget(&root);
        root.show_all();

        let menu = gtk::Menu::new();
        root.add_events(gtk::gdk::EventMask::BUTTON_PRESS_MASK);
        let weak_for_click = Rc::downgrade(&runtime);
        root.connect_button_press_event(move |_, event| {
            if event.button() != 1 && event.button() != 3 {
                return glib::Propagation::Proceed;
            }
            if let Some(runtime) = weak_for_click.upgrade() {
                rebuild_menu(&runtime);
                if let Some(menu) = runtime.borrow().menu.as_ref() {
                    menu.popup_easy(event.button(), event.time());
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        runtime.borrow_mut().root = Some(root);
        runtime.borrow_mut().icon = Some(icon);
        runtime.borrow_mut().label = Some(label);
        runtime.borrow_mut().menu = Some(menu);
        *state = Rc::clone(&runtime);

        match connect_bus() {
            Ok(connection) => {
                runtime.borrow_mut().connection = Some(connection.clone());
                let weak_for_signal = Rc::downgrade(&runtime);
                let subscription = connection.subscribe_to_signal(
                    Some(DBUS_SERVICE),
                    Some(DBUS_INTERFACE),
                    Some("SnapshotChanged"),
                    Some(DBUS_PATH),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |_signal| {
                        if let Some(runtime) = weak_for_signal.upgrade() {
                            fetch_snapshot(runtime);
                        }
                    },
                );
                runtime.borrow_mut().subscription = Some(subscription);
                fetch_snapshot(Rc::clone(&runtime));

                let weak_timer = Rc::downgrade(&runtime);
                glib::timeout_add_seconds_local(5, move || {
                    if let Some(runtime) = weak_timer.upgrade() {
                        fetch_snapshot(runtime);
                        glib::ControlFlow::Continue
                    } else {
                        glib::ControlFlow::Break
                    }
                });
            }
            Err(error) => {
                set_unavailable(&runtime, &format!("无法连接 D-Bus: {error}"));
            }
        }
    }

    fn size_changed(_state: &mut Self::State, _plugin: &Plugin, _size: i32) -> bool {
        false
    }

    fn free_data(state: &mut Self::State, _plugin: &Plugin) {
        state.borrow_mut().subscription.take();
    }
}

fn connect_bus() -> Result<gio::DBusConnection, glib::Error> {
    let bus_type = if std::env::var("NETCORE_BUS").as_deref() == Ok("session") {
        gio::BusType::Session
    } else {
        gio::BusType::System
    };
    gio::bus_get_sync(bus_type, None::<&gio::Cancellable>)
}

fn fetch_snapshot(runtime: Shared) {
    let connection = match runtime.borrow().connection.clone() {
        Some(connection) => connection,
        None => return,
    };
    let generation = {
        let mut borrow = runtime.borrow_mut();
        borrow.generation = borrow.generation.wrapping_add(1);
        borrow.generation
    };
    let weak = Rc::downgrade(&runtime);
    connection.call(
        Some(DBUS_SERVICE),
        DBUS_PATH,
        DBUS_INTERFACE,
        "GetSnapshot",
        None,
        None,
        gio::DBusCallFlags::NONE,
        3_000,
        None::<&gio::Cancellable>,
        move |result| {
            let Some(runtime) = weak.upgrade() else {
                return;
            };
            if runtime.borrow().generation != generation {
                return;
            }
            match result {
                Ok(variant) => match variant.get::<(String,)>() {
                    Some((json,)) => match serde_json::from_str::<ManagerSnapshot>(&json) {
                        Ok(snapshot) => {
                            {
                                let mut borrow = runtime.borrow_mut();
                                borrow.snapshot = Some(snapshot.clone());
                                borrow.available = true;
                                borrow.last_error = None;
                            }
                            render_panel(&runtime, &snapshot);
                        }
                        Err(error) => {
                            set_unavailable(&runtime, &format!("解析状态失败: {error}"));
                        }
                    },
                    None => set_unavailable(&runtime, "守护进程返回了无效的 GetSnapshot 响应"),
                },
                Err(error) => {
                    set_unavailable(&runtime, &format!("netcore 守护进程不可用: {error}"));
                }
            }
        },
    );
}

fn render_panel(runtime: &Shared, snapshot: &ManagerSnapshot) {
    let icon_name = if snapshot.state == ManagerState::Connecting {
        "network-wireless-acquiring-symbolic"
    } else if snapshot
        .devices
        .iter()
        .any(|device| device.kind == DeviceKind::Wifi && device.state == DeviceState::Connected)
    {
        "network-wireless-symbolic"
    } else if snapshot
        .devices
        .iter()
        .any(|device| device.state == DeviceState::Connected)
    {
        "network-wired-symbolic"
    } else {
        "network-offline-symbolic"
    };
    let label = snapshot
        .devices
        .iter()
        .find(|device| device.state == DeviceState::Connected)
        .map_or_else(
            || "离线".to_owned(),
            |device| match device.kind {
                DeviceKind::Wifi => device.ssid.clone().unwrap_or_else(|| device.ifname.clone()),
                _ => "有线".to_owned(),
            },
        );
    let detail = snapshot
        .devices
        .iter()
        .find(|device| device.state == DeviceState::Connected)
        .map_or_else(|| "netcore: 无活动连接".to_owned(), describe_device);
    let runtime = runtime.borrow();
    if let Some(icon) = &runtime.icon {
        icon.set_from_icon_name(Some(icon_name), gtk::IconSize::Menu);
    }
    if let Some(label_widget) = &runtime.label {
        label_widget.set_text(&truncate(&label, 16));
    }
    if let Some(root) = &runtime.root {
        root.set_tooltip_text(Some(&detail));
    }
}

fn set_unavailable(runtime: &Shared, message: &str) {
    {
        let mut borrow = runtime.borrow_mut();
        if borrow.available || borrow.last_error.as_deref() != Some(message) {
            borrow.available = false;
            borrow.snapshot = None;
            borrow.last_error = Some(message.to_owned());
        }
    }
    let runtime = runtime.borrow();
    if let Some(icon) = &runtime.icon {
        icon.set_from_icon_name(Some("network-offline-symbolic"), gtk::IconSize::Menu);
    }
    if let Some(label) = &runtime.label {
        label.set_text("不可用");
        label.set_tooltip_text(Some(message));
    }
    if let Some(root) = &runtime.root {
        root.set_tooltip_text(Some(message));
    }
}

fn rebuild_menu(runtime: &Shared) {
    let snapshot = runtime.borrow().snapshot.clone();
    let available = runtime.borrow().available;
    let last_error = runtime.borrow().last_error.clone();
    let menu = match runtime.borrow().menu.clone() {
        Some(menu) => menu,
        None => return,
    };
    for child in menu.children() {
        menu.remove(&child);
    }

    let header = gtk::MenuItem::with_label(&snapshot.as_ref().map_or_else(
        || "netcore: 守护进程不可用".to_owned(),
        |snapshot| {
            format!(
                "{} · {}",
                snapshot.state.label(),
                snapshot.connectivity.label()
            )
        },
    ));
    header.set_sensitive(false);
    menu.append(&header);

    if let Some(error) = &last_error {
        let item = gtk::MenuItem::with_label(&truncate(error, 70));
        item.set_sensitive(false);
        menu.append(&item);
    }

    if let Some(snapshot) = &snapshot {
        let wifi_switch = gtk::CheckMenuItem::with_label("Wi-Fi");
        wifi_switch.set_active(snapshot.wifi_enabled);
        wifi_switch.set_sensitive(available && snapshot.wifi_available);
        let enabled = snapshot.wifi_enabled;
        let weak = Rc::downgrade(runtime);
        wifi_switch.connect_activate(move |_| {
            if let Some(runtime) = weak.upgrade() {
                call_action(
                    runtime,
                    "SetWifiEnabled",
                    Some((!enabled,).to_variant()),
                    15_000,
                );
            }
        });
        menu.append(&wifi_switch);

        if let Some(device) = snapshot
            .devices
            .iter()
            .find(|device| device.kind == DeviceKind::Wifi)
        {
            let networks_item =
                gtk::MenuItem::with_label(&format!("Wi-Fi 网络 ({})", device.networks.len()));
            let networks_menu = gtk::Menu::new();
            build_network_menu(&networks_menu, runtime, device);
            networks_item.set_submenu(Some(&networks_menu));
            menu.append(&networks_item);
        }

        let devices_item = gtk::MenuItem::with_label("网络设备");
        let devices_menu = gtk::Menu::new();
        build_device_menu(&devices_menu, runtime, &snapshot.devices);
        devices_item.set_submenu(Some(&devices_menu));
        menu.append(&devices_item);

        menu.append(&gtk::SeparatorMenuItem::new());

        let scan = gtk::MenuItem::with_label("扫描 Wi-Fi");
        scan.set_sensitive(available && snapshot.wifi_available);
        let weak = Rc::downgrade(runtime);
        let ifname = snapshot
            .devices
            .iter()
            .find(|device| device.kind == DeviceKind::Wifi)
            .map(|device| device.ifname.clone());
        scan.connect_activate(move |_| {
            if let (Some(runtime), Some(ifname)) = (weak.upgrade(), ifname.clone()) {
                call_action(runtime, "ScanWifi", Some((ifname,).to_variant()), 30_000);
            }
        });
        menu.append(&scan);
    }

    let hidden = gtk::MenuItem::with_label("加入隐藏网络…");
    hidden.set_sensitive(available);
    let weak = Rc::downgrade(runtime);
    hidden.connect_activate(move |_| {
        if let Some(runtime) = weak.upgrade() {
            show_password_dialog(runtime, String::new(), true);
        }
    });
    menu.append(&hidden);

    let open = gtk::MenuItem::with_label("打开网络管理器");
    open.connect_activate(|_| {
        let _ = std::process::Command::new("netcore-gui").spawn();
    });
    menu.append(&open);

    menu.show_all();
}

fn build_network_menu(menu: &gtk::Menu, runtime: &Shared, device: &NetworkDevice) {
    let mut networks = device.networks.clone();
    networks.sort_by_key(|network| std::cmp::Reverse(network.signal));
    if networks.is_empty() {
        let item = gtk::MenuItem::with_label("未发现网络，点击扫描");
        item.set_sensitive(false);
        menu.append(&item);
        return;
    }
    for network in networks.into_iter().take(12) {
        let prefix = if network.connected { "✓ " } else { "" };
        let label = format!(
            "{prefix}{}  {}  {}%",
            network.ssid,
            network.security.label(),
            network.quality_percent()
        );
        let item = gtk::MenuItem::with_label(&label);
        item.set_sensitive(!network.connected);
        let ifname = device.ifname.clone();
        let weak = Rc::downgrade(runtime);
        item.connect_activate(move |_| {
            let Some(runtime) = weak.upgrade() else {
                return;
            };
            if network.security.requires_password() {
                show_password_dialog(runtime, network.ssid.clone(), false);
            } else {
                connect_wifi(
                    runtime,
                    ifname.clone(),
                    network.ssid.clone(),
                    String::new(),
                    false,
                );
            }
        });
        menu.append(&item);
    }
}

fn build_device_menu(menu: &gtk::Menu, runtime: &Shared, devices: &[NetworkDevice]) {
    if devices.is_empty() {
        let item = gtk::MenuItem::with_label("没有可管理的接口");
        item.set_sensitive(false);
        menu.append(&item);
        return;
    }
    for device in devices {
        let item =
            gtk::MenuItem::with_label(&format!("{} · {}", device.ifname, device.state.label()));
        item.set_sensitive(false);
        menu.append(&item);
        let (label, desired) = match device.state {
            DeviceState::Connected => ("断开", false),
            DeviceState::Disconnected | DeviceState::Failed => ("连接", true),
            _ => continue,
        };
        let action = gtk::MenuItem::with_label(&format!("    {label} {}", device.ifname));
        let ifname = device.ifname.clone();
        let weak = Rc::downgrade(runtime);
        action.connect_activate(move |_| {
            if let Some(runtime) = weak.upgrade() {
                if desired {
                    call_action(
                        runtime,
                        "SetDeviceEnabled",
                        Some((ifname.clone(), true).to_variant()),
                        60_000,
                    );
                } else {
                    call_action(
                        runtime,
                        "DisconnectDevice",
                        Some((ifname.clone(),).to_variant()),
                        15_000,
                    );
                }
            }
        });
        menu.append(&action);
    }
}

fn show_password_dialog(runtime: Shared, ssid: String, hidden: bool) {
    let previous = runtime.borrow().snapshot.clone();
    let ifname = previous
        .as_ref()
        .and_then(|snapshot| {
            snapshot
                .devices
                .iter()
                .find(|device| device.kind == DeviceKind::Wifi)
                .map(|device| device.ifname.clone())
        })
        .unwrap_or_else(|| "wlan0".to_owned());
    let dialog = gtk::Dialog::with_buttons(
        Some(if hidden {
            "加入隐藏网络"
        } else {
            "连接到 Wi-Fi"
        }),
        None::<&gtk::Window>,
        gtk::DialogFlags::MODAL,
        &[
            ("取消", gtk::ResponseType::Cancel),
            ("连接", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_default_size(380, -1);
    dialog.set_default_response(gtk::ResponseType::Accept);
    let area = dialog.content_area();
    area.set_spacing(8);
    area.set_border_width(12);
    let ssid_label = gtk::Label::new(Some("网络名称 (SSID)"));
    ssid_label.set_xalign(0.0);
    let ssid_entry = gtk::Entry::new();
    ssid_entry.set_text(&ssid);
    ssid_entry.set_editable(hidden);
    ssid_entry.set_placeholder_text(Some("Wi-Fi 名称"));
    let password_label = gtk::Label::new(Some(if hidden {
        "密码（开放网络可留空）"
    } else {
        "密码"
    }));
    password_label.set_xalign(0.0);
    let password_entry = gtk::Entry::new();
    password_entry.set_visibility(false);
    password_entry.set_input_purpose(gtk::InputPurpose::Password);
    password_entry.set_placeholder_text(Some("Wi-Fi 密码"));
    password_entry.set_activates_default(true);
    area.add(&ssid_label);
    area.add(&ssid_entry);
    area.add(&password_label);
    area.add(&password_entry);
    if hidden {
        ssid_entry.grab_focus();
    } else {
        password_entry.grab_focus();
    }
    dialog.show_all();
    let weak = Rc::downgrade(&runtime);
    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(runtime) = weak.upgrade()
        {
            let ssid = ssid_entry.text().to_string();
            let password = password_entry.text().to_string();
            if !ssid.trim().is_empty() {
                connect_wifi(runtime, ifname.clone(), ssid, password, hidden);
            }
        }
        dialog.close();
    });
}

fn connect_wifi(runtime: Shared, ifname: String, ssid: String, password: String, hidden: bool) {
    call_action(
        runtime,
        "ConnectWifi",
        Some((ifname, ssid, password, hidden).to_variant()),
        90_000,
    );
}

fn call_action(
    runtime: Shared,
    method: &'static str,
    parameters: Option<glib::Variant>,
    timeout_ms: i32,
) {
    let Some(connection) = runtime.borrow().connection.clone() else {
        show_error("无法连接 D-Bus");
        return;
    };
    let weak = Rc::downgrade(&runtime);
    connection.call(
        Some(DBUS_SERVICE),
        DBUS_PATH,
        DBUS_INTERFACE,
        method,
        parameters.as_ref(),
        None,
        gio::DBusCallFlags::NONE,
        timeout_ms,
        None::<&gio::Cancellable>,
        move |result| match result {
            Ok(_) => {
                if let Some(runtime) = weak.upgrade() {
                    fetch_snapshot(runtime);
                }
            }
            Err(error) => {
                show_error(&format!("{method} 失败: {error}"));
                if let Some(runtime) = weak.upgrade() {
                    fetch_snapshot(runtime);
                }
            }
        },
    );
}

fn show_error(message: &str) {
    let dialog = gtk::MessageDialog::new(
        None::<&gtk::Window>,
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Error,
        gtk::ButtonsType::Close,
        message,
    );
    dialog.set_title("netcore");
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.show_all();
}

fn describe_device(device: &NetworkDevice) -> String {
    match device.kind {
        DeviceKind::Wifi => format!(
            "netcore: {} · {} · {}",
            device.state.label(),
            device.ssid.as_deref().unwrap_or("-"),
            device.primary_ipv4().unwrap_or("无地址")
        ),
        _ => format!(
            "netcore: {} · {} · {}",
            device.state.label(),
            device.ifname,
            device.primary_ipv4().unwrap_or("无地址")
        ),
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut result = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        result.push('…');
    }
    result
}

xfce_panel::define_panel_plugin!(NetcorePlugin);
