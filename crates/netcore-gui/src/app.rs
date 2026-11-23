//! GTK widget tree and state rendering.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Button, HeaderBar, Image, Label, Orientation,
    Revealer, ScrolledWindow, Spinner,
};
use gtk4 as gtk;
use gtk4::glib;
use netcore_client::BusKind;
use netcore_common::{
    DeviceKind, DeviceState, ManagerSnapshot, ManagerState, NetworkDevice, Security, WifiNetwork,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::client::{self, Request, UiEvent};

const CSS: &str = "
.netcore-hero { padding: 18px; }
.netcore-device { padding: 14px; }
.netcore-network-row { padding: 10px 12px; border-radius: 10px; }
.netcore-banner { padding: 10px 12px; }
.netcore-banner-error { background-color: alpha(#e01b24, 0.16); }
.netcore-banner-success { background-color: alpha(#2ec27e, 0.18); }
";

#[derive(Default)]
struct AppState {
    snapshot: Option<ManagerSnapshot>,
    daemon_available: bool,
    updating_switch: bool,
    scanning: bool,
    last_key: Option<String>,
}

struct Ui {
    window: ApplicationWindow,
    scrolled: ScrolledWindow,
    header_subtitle: Label,
    hero_icon: Image,
    hero_title: Label,
    hero_detail: Label,
    hero_spinner: Spinner,
    wifi_switch: gtk::Switch,
    devices_box: GtkBox,
    wifi_box: GtkBox,
    wifi_hint: Label,
    scan_button: Button,
    hidden_button: Button,
    banner: Revealer,
    banner_box: GtkBox,
    banner_label: Label,
    banner_generation: Rc<Cell<u32>>,
}

pub fn run(bus: BusKind) {
    let application = Application::builder()
        .application_id("org.netcore.NetworkManager.Gui")
        .build();
    application.connect_activate(move |application| {
        build_window(application, bus);
    });
    application.run();
}

fn build_window(application: &Application, bus: BusKind) {
    install_css();

    let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    client::spawn(bus, request_rx, event_tx);
    set_request_tx(request_tx.clone());

    let window = ApplicationWindow::builder()
        .application(application)
        .title("网络")
        .default_width(880)
        .default_height(780)
        .build();

    let header = HeaderBar::new();
    let title_box = GtkBox::new(Orientation::Vertical, 0);
    let title = Label::new(Some("网络"));
    title.add_css_class("title-4");
    title.set_xalign(0.0);
    let subtitle = Label::new(Some("正在连接 netcore…"));
    subtitle.add_css_class("dim-label");
    subtitle.add_css_class("caption");
    subtitle.set_xalign(0.0);
    title_box.append(&title);
    title_box.append(&subtitle);
    header.set_title_widget(Some(&title_box));

    let refresh_button = icon_button("view-refresh-symbolic", "刷新");
    header.pack_end(&refresh_button);
    let hidden_button = Button::with_label("隐藏网络");
    hidden_button.add_css_class("flat");
    hidden_button.set_tooltip_text(Some("加入隐藏的 Wi-Fi 网络"));
    header.pack_end(&hidden_button);
    window.set_titlebar(Some(&header));

    let content = GtkBox::new(Orientation::Vertical, 14);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(22);
    content.set_margin_end(22);

    let hero = GtkBox::new(Orientation::Horizontal, 16);
    hero.add_css_class("card");
    hero.add_css_class("netcore-hero");
    let hero_icon = Image::from_icon_name("network-offline-symbolic");
    hero_icon.set_pixel_size(46);
    hero.append(&hero_icon);
    let hero_text = GtkBox::new(Orientation::Vertical, 3);
    hero_text.set_hexpand(true);
    let hero_title = Label::new(Some("正在连接守护进程…"));
    hero_title.add_css_class("title-2");
    hero_title.set_xalign(0.0);
    let hero_detail = Label::new(Some("netcore 网络管理器"));
    hero_detail.add_css_class("dim-label");
    hero_detail.set_xalign(0.0);
    hero_detail.set_wrap(true);
    hero_detail.set_max_width_chars(60);
    let hero_spinner = Spinner::new();
    hero_spinner.set_visible(false);
    hero_text.append(&hero_title);
    hero_text.append(&hero_detail);
    hero_text.append(&hero_spinner);
    hero.append(&hero_text);

    let switch_box = GtkBox::new(Orientation::Vertical, 4);
    switch_box.set_valign(gtk::Align::Center);
    let wifi_label = Label::new(Some("Wi-Fi"));
    wifi_label.add_css_class("caption");
    wifi_label.add_css_class("dim-label");
    let wifi_switch = gtk::Switch::new();
    wifi_switch.set_valign(gtk::Align::Center);
    wifi_switch.set_tooltip_text(Some("启用或禁用 Wi-Fi"));
    switch_box.append(&wifi_label);
    switch_box.append(&wifi_switch);
    hero.append(&switch_box);
    content.append(&hero);

    let devices_title = section_label("网络设备");
    content.append(&devices_title);
    let devices_box = GtkBox::new(Orientation::Vertical, 8);
    content.append(&devices_box);

    let wifi_header = GtkBox::new(Orientation::Horizontal, 8);
    let wifi_title = section_label("Wi-Fi 网络");
    wifi_title.set_hexpand(true);
    let scan_button = Button::with_label("扫描");
    scan_button.add_css_class("flat");
    scan_button.set_tooltip_text(Some("扫描附近 Wi-Fi"));
    wifi_header.append(&wifi_title);
    wifi_header.append(&scan_button);
    content.append(&wifi_header);

    let wifi_hint = Label::new(Some("尚未扫描"));
    wifi_hint.add_css_class("dim-label");
    wifi_hint.set_xalign(0.0);
    wifi_hint.set_wrap(true);
    content.append(&wifi_hint);
    let wifi_box = GtkBox::new(Orientation::Vertical, 6);
    content.append(&wifi_box);

    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();

    let banner_label = Label::new(None);
    banner_label.set_xalign(0.0);
    banner_label.set_wrap(true);
    banner_label.set_hexpand(true);
    let banner_close = icon_button("window-close-symbolic", "关闭");
    banner_close.add_css_class("flat");
    let banner_box = GtkBox::new(Orientation::Horizontal, 8);
    banner_box.add_css_class("card");
    banner_box.add_css_class("netcore-banner");
    banner_box.append(&banner_label);
    banner_box.append(&banner_close);
    let banner = Revealer::new();
    banner.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    banner.set_reveal_child(false);
    banner.set_child(Some(&banner_box));
    banner.set_margin_start(22);
    banner.set_margin_end(22);
    banner.set_margin_bottom(12);

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&scrolled);
    root.append(&banner);
    window.set_child(Some(&root));

    let ui = Rc::new(Ui {
        window: window.clone(),
        scrolled: scrolled.clone(),
        header_subtitle: subtitle,
        hero_icon,
        hero_title,
        hero_detail,
        hero_spinner,
        wifi_switch: wifi_switch.clone(),
        devices_box,
        wifi_box,
        wifi_hint,
        scan_button: scan_button.clone(),
        hidden_button: hidden_button.clone(),
        banner,
        banner_box,
        banner_label,
        banner_generation: Rc::new(Cell::new(0)),
    });
    let state = Rc::new(RefCell::new(AppState::default()));

    let banner_for_close = ui.banner.clone();
    banner_close.connect_clicked(move |_| banner_for_close.set_reveal_child(false));

    let state_for_switch = Rc::clone(&state);
    let tx_for_switch = request_tx.clone();
    wifi_switch.connect_active_notify(move |switch| {
        if state_for_switch.borrow().updating_switch {
            return;
        }
        let _ = tx_for_switch.send(Request::SetWifiEnabled {
            enabled: switch.is_active(),
        });
    });

    let tx_for_refresh = request_tx.clone();
    refresh_button.connect_clicked(move |_| {
        let _ = tx_for_refresh.send(Request::Refresh);
    });

    let state_for_scan = Rc::clone(&state);
    let ui_for_scan = Rc::clone(&ui);
    let tx_for_scan = request_tx.clone();
    scan_button.connect_clicked(move |_| {
        let Some(ifname) = wifi_ifname(&state_for_scan.borrow()) else {
            show_banner(&ui_for_scan, true, "没有可用的 Wi-Fi 接口");
            return;
        };
        state_for_scan.borrow_mut().scanning = true;
        ui_for_scan.scan_button.set_sensitive(false);
        ui_for_scan.scan_button.set_label("扫描中…");
        let _ = tx_for_scan.send(Request::Scan { ifname });
    });

    let state_for_hidden = Rc::clone(&state);
    let ui_for_hidden = Rc::clone(&ui);
    let tx_for_hidden = request_tx.clone();
    hidden_button.connect_clicked(move |_| {
        let Some(ifname) = wifi_ifname(&state_for_hidden.borrow()) else {
            show_banner(&ui_for_hidden, true, "没有可用的 Wi-Fi 接口");
            return;
        };
        show_connect_dialog(
            &ui_for_hidden.window,
            ifname,
            String::new(),
            true,
            tx_for_hidden.clone(),
        );
    });

    let ui_for_events = Rc::clone(&ui);
    let state_for_events = Rc::clone(&state);
    let request_tx_for_events = request_tx.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        loop {
            match event_rx.try_recv() {
                Ok(event) => handle_event(
                    &ui_for_events,
                    &state_for_events,
                    event,
                    &request_tx_for_events,
                ),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        glib::ControlFlow::Continue
    });

    window.present();
}

fn handle_event(
    ui: &Rc<Ui>,
    state: &Rc<RefCell<AppState>>,
    event: UiEvent,
    request_tx: &UnboundedSender<Request>,
) {
    match event {
        UiEvent::Connecting => {
            state.borrow_mut().daemon_available = false;
            ui.hero_spinner.set_visible(true);
            ui.hero_spinner.start();
            ui.hero_title.set_text("正在连接守护进程…");
            ui.hero_detail.set_text("netcore 网络管理器");
            ui.wifi_switch.set_sensitive(false);
            ui.hidden_button.set_sensitive(false);
        }
        UiEvent::Snapshot(snapshot) => {
            ui.hero_spinner.stop();
            ui.hero_spinner.set_visible(false);
            state.borrow_mut().daemon_available = true;
            state.borrow_mut().scanning = false;
            ui.scan_button.set_label("扫描");
            ui.scan_button.set_sensitive(snapshot.wifi_available);
            ui.hidden_button.set_sensitive(true);
            render_snapshot(ui, state, *snapshot);
        }
        UiEvent::DaemonUnavailable(message) => {
            state.borrow_mut().daemon_available = false;
            state.borrow_mut().snapshot = None;
            state.borrow_mut().last_key = None;
            state.borrow_mut().scanning = false;
            ui.hero_spinner.stop();
            ui.hero_spinner.set_visible(false);
            ui.hero_icon.set_icon_name(Some("network-offline-symbolic"));
            ui.hero_title.set_text("守护进程不可用");
            ui.hero_detail.set_text(&message);
            ui.header_subtitle.set_text("无法连接 netcore");
            ui.wifi_switch.set_sensitive(false);
            ui.scan_button.set_sensitive(false);
            ui.hidden_button.set_sensitive(false);
            ui.scan_button.set_label("扫描");
            clear_box(&ui.devices_box);
            clear_box(&ui.wifi_box);
            ui.wifi_hint.set_text("等待 netcore 守护进程启动…");
            show_banner(ui, true, &message);
        }
        UiEvent::Busy(busy) => {
            ui.hero_spinner.set_visible(busy);
            if busy {
                ui.hero_spinner.start();
            } else {
                ui.hero_spinner.stop();
            }
        }
        UiEvent::ActionFinished(message) => {
            ui.hero_spinner.stop();
            ui.hero_spinner.set_visible(false);
            show_banner(ui, false, &message);
            let _ = request_tx.send(Request::Refresh);
        }
        UiEvent::Error(message) => {
            ui.hero_spinner.stop();
            ui.hero_spinner.set_visible(false);
            state.borrow_mut().scanning = false;
            ui.scan_button.set_sensitive(true);
            ui.scan_button.set_label("扫描");
            show_banner(ui, true, &message);
        }
    }
}

fn render_snapshot(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>, snapshot: ManagerSnapshot) {
    let scroll_value = ui.scrolled.vadjustment().value();
    let key = {
        let mut key_snapshot = snapshot.clone();
        key_snapshot.updated_at = 0;
        key_snapshot.to_json()
    };
    if state.borrow().last_key.as_deref() == Some(key.as_str()) {
        return;
    }
    state.borrow_mut().last_key = Some(key);
    state.borrow_mut().snapshot = Some(snapshot.clone());
    ui.scan_button.set_sensitive(snapshot.wifi_available);
    ui.wifi_switch
        .set_sensitive(snapshot.wifi_available && snapshot.wpa_backend != "unavailable");
    state.borrow_mut().updating_switch = true;
    ui.wifi_switch.set_active(snapshot.wifi_enabled);
    state.borrow_mut().updating_switch = false;

    ui.header_subtitle.set_text(&format!(
        "{} · netcore {} · {}",
        snapshot.hostname, snapshot.version, snapshot.wpa_backend
    ));
    ui.hero_icon
        .set_icon_name(Some(hero_icon(&snapshot, &snapshot.devices)));
    ui.hero_title.set_text(snapshot.state.label());
    let active = snapshot
        .devices
        .iter()
        .filter(|device| device.state == DeviceState::Connected)
        .map(describe_connection)
        .collect::<Vec<_>>();
    ui.hero_detail.set_text(&if active.is_empty() {
        format!("{} · 没有活动连接", snapshot.connectivity.label())
    } else {
        format!(
            "{} · {}",
            snapshot.connectivity.label(),
            active.join("  ·  ")
        )
    });

    clear_box(&ui.devices_box);
    for device in &snapshot.devices {
        let card = build_device_card(ui, device, &snapshot);
        ui.devices_box.append(&card);
    }
    if snapshot.devices.is_empty() {
        ui.devices_box.append(&muted_label("没有可管理的网络接口"));
    }

    clear_box(&ui.wifi_box);
    let wifi_device = snapshot
        .devices
        .iter()
        .find(|device| device.kind == DeviceKind::Wifi);
    let Some(wifi_device) = wifi_device else {
        ui.wifi_hint.set_text("没有 Wi-Fi 设备");
        return;
    };
    if !snapshot.wifi_available {
        ui.wifi_hint
            .set_text("wpa_supplicant 后端不可用；请检查 wpa_supplicant 服务");
        return;
    }
    if !snapshot.wifi_enabled {
        ui.wifi_hint.set_text("Wi-Fi 已禁用");
        return;
    }
    if wifi_device.networks.is_empty() {
        ui.wifi_hint
            .set_text("暂无扫描结果，点击“扫描”查找附近网络");
        return;
    }
    ui.wifi_hint.set_text(&format!(
        "{} 个网络 · 点击网络进行连接",
        wifi_device.networks.len()
    ));
    for network in &wifi_device.networks {
        let row = build_network_row(ui, wifi_device, network, &snapshot);
        ui.wifi_box.append(&row);
    }
    let adjustment = ui.scrolled.vadjustment();
    glib::idle_add_local_once(move || adjustment.set_value(scroll_value));
}

fn build_device_card(_ui: &Rc<Ui>, device: &NetworkDevice, snapshot: &ManagerSnapshot) -> GtkBox {
    let card = GtkBox::new(Orientation::Horizontal, 12);
    card.add_css_class("card");
    card.add_css_class("netcore-device");
    let icon = Image::from_icon_name(device_icon(device));
    icon.set_pixel_size(28);
    icon.set_valign(gtk::Align::Center);
    card.append(&icon);

    let text = GtkBox::new(Orientation::Vertical, 2);
    text.set_hexpand(true);
    let name = Label::new(Some(&device_name(device)));
    name.add_css_class("heading");
    name.set_xalign(0.0);
    let state_text = Label::new(Some(&device_description(device)));
    state_text.add_css_class("dim-label");
    state_text.set_xalign(0.0);
    state_text.set_wrap(true);
    let meta = Label::new(Some(&device_meta(device)));
    meta.add_css_class("caption");
    meta.add_css_class("dim-label");
    meta.set_xalign(0.0);
    meta.set_wrap(true);
    text.append(&name);
    text.append(&state_text);
    text.append(&meta);
    card.append(&text);

    let action = match device.state {
        DeviceState::Connected => Some(("断开", false)),
        DeviceState::Disconnecting => None,
        DeviceState::Connecting | DeviceState::IpConfig => None,
        _ if device.kind == DeviceKind::Ethernet => Some(("连接", true)),
        _ => None,
    };
    if let Some((label, enabled)) = action {
        let button = Button::with_label(label);
        button.add_css_class("flat");
        button.set_valign(gtk::Align::Center);
        button.set_sensitive(
            snapshot.wpa_backend != "unavailable" || device.kind != DeviceKind::Wifi,
        );
        let tx = REQUEST_TX.with(|tx| tx.borrow().clone());
        if let Some(tx) = tx {
            let ifname = device.ifname.clone();
            button.connect_clicked(move |_| {
                if enabled {
                    let _ = tx.send(Request::SetDeviceEnabled {
                        ifname: ifname.clone(),
                        enabled: true,
                    });
                } else {
                    let _ = tx.send(Request::Disconnect {
                        ifname: ifname.clone(),
                    });
                }
            });
        }
        card.append(&button);
    }
    card
}

fn build_network_row(
    ui: &Rc<Ui>,
    device: &NetworkDevice,
    network: &WifiNetwork,
    _snapshot: &ManagerSnapshot,
) -> Button {
    let row = Button::new();
    row.add_css_class("flat");
    row.add_css_class("netcore-network-row");
    let content = GtkBox::new(Orientation::Horizontal, 10);
    let icon = Image::from_icon_name(signal_icon(network.quality_percent()));
    icon.set_pixel_size(22);
    icon.set_valign(gtk::Align::Center);
    content.append(&icon);
    let text = GtkBox::new(Orientation::Vertical, 1);
    text.set_hexpand(true);
    let ssid = Label::new(Some(&network.ssid));
    ssid.set_xalign(0.0);
    ssid.add_css_class("heading");
    let security = Label::new(Some(if network.security == Security::Open {
        "开放网络"
    } else {
        network.security.label()
    }));
    security.set_xalign(0.0);
    security.add_css_class("caption");
    security.add_css_class("dim-label");
    text.append(&ssid);
    text.append(&security);
    content.append(&text);
    let percent = Label::new(Some(&format!("{}%", network.quality_percent())));
    percent.add_css_class("caption");
    percent.add_css_class("dim-label");
    percent.add_css_class("netcore-percent");
    content.append(&percent);
    if network.connected {
        let badge = Label::new(Some("已连接"));
        badge.add_css_class("caption");
        badge.add_css_class("success");
        content.append(&badge);
        row.set_sensitive(false);
    } else if network.security != Security::Open {
        let lock = Image::from_icon_name("changes-prevent-symbolic");
        lock.set_pixel_size(14);
        content.append(&lock);
    }
    row.set_child(Some(&content));

    if network.connected {
        return row;
    }
    let tx = REQUEST_TX.with(|tx| tx.borrow().clone());
    let Some(tx) = tx else {
        return row;
    };
    let ifname = device.ifname.clone();
    let ssid_text = network.ssid.clone();
    let secured = network.security != Security::Open;
    let window = ui.window.clone();
    row.connect_clicked(move |_| {
        if secured {
            show_connect_dialog(
                &window,
                ifname.clone(),
                ssid_text.clone(),
                false,
                tx.clone(),
            );
        } else {
            let _ = tx.send(Request::ConnectWifi {
                ifname: ifname.clone(),
                ssid: ssid_text.clone(),
                password: String::new(),
                hidden: false,
            });
        }
    });
    row
}

thread_local! {
    static REQUEST_TX: RefCell<Option<UnboundedSender<Request>>> = const { RefCell::new(None) };
}

fn set_request_tx(tx: UnboundedSender<Request>) {
    REQUEST_TX.with(|cell| *cell.borrow_mut() = Some(tx));
}

fn show_connect_dialog(
    parent: &ApplicationWindow,
    ifname: String,
    initial_ssid: String,
    hidden: bool,
    tx: UnboundedSender<Request>,
) {
    let dialog = gtk::Window::builder()
        .title(if hidden {
            "加入隐藏网络"
        } else {
            "连接到 Wi-Fi"
        })
        .transient_for(parent)
        .modal(true)
        .resizable(false)
        .default_width(400)
        .build();
    let header = HeaderBar::new();
    header.set_show_title_buttons(true);
    dialog.set_titlebar(Some(&header));

    let content = GtkBox::new(Orientation::Vertical, 12);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(20);
    content.set_margin_end(20);
    let ssid_label = Label::new(Some("网络名称 (SSID)"));
    ssid_label.set_xalign(0.0);
    ssid_label.add_css_class("caption");
    ssid_label.add_css_class("dim-label");
    let ssid_entry = gtk::Entry::new();
    ssid_entry.set_text(&initial_ssid);
    ssid_entry.set_editable(hidden);
    ssid_entry.set_placeholder_text(Some("Wi-Fi 名称"));
    let password_label = Label::new(Some(if hidden {
        "密码（开放网络可留空）"
    } else {
        "密码"
    }));
    password_label.set_xalign(0.0);
    password_label.add_css_class("caption");
    password_label.add_css_class("dim-label");
    let password_entry = gtk::PasswordEntry::new();
    password_entry.set_show_peek_icon(true);
    password_entry.set_placeholder_text(Some("Wi-Fi 密码"));
    let error_label = Label::new(None);
    error_label.add_css_class("error");
    error_label.set_xalign(0.0);
    error_label.set_wrap(true);
    content.append(&ssid_label);
    content.append(&ssid_entry);
    content.append(&password_label);
    content.append(&password_entry);
    content.append(&error_label);

    let buttons = GtkBox::new(Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(4);
    let cancel = Button::with_label("取消");
    let connect = Button::with_label("连接");
    connect.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&connect);
    content.append(&buttons);
    dialog.set_child(Some(&content));

    let dialog_for_cancel = dialog.clone();
    cancel.connect_clicked(move |_| dialog_for_cancel.close());

    let dialog_for_connect = dialog.clone();
    connect.connect_clicked(move |_| {
        let ssid = ssid_entry.text().to_string();
        if ssid.trim().is_empty() {
            error_label.set_text("请输入网络名称");
            return;
        }
        let password = password_entry.text().to_string();
        let _ = tx.send(Request::ConnectWifi {
            ifname: ifname.clone(),
            ssid,
            password,
            hidden,
        });
        dialog_for_connect.close();
    });
    dialog.present();
}

fn wifi_ifname(state: &AppState) -> Option<String> {
    state
        .snapshot
        .as_ref()?
        .devices
        .iter()
        .find(|device| device.kind == DeviceKind::Wifi)
        .map(|device| device.ifname.clone())
}

fn show_banner(ui: &Ui, error: bool, message: &str) {
    ui.banner_label.set_text(message);
    ui.banner_box.remove_css_class("netcore-banner-error");
    ui.banner_box.remove_css_class("netcore-banner-success");
    ui.banner_box.add_css_class(if error {
        "netcore-banner-error"
    } else {
        "netcore-banner-success"
    });
    ui.banner.set_reveal_child(true);
    let generation = ui.banner_generation.get().wrapping_add(1);
    ui.banner_generation.set(generation);
    let banner = ui.banner.clone();
    let generation_cell = Rc::clone(&ui.banner_generation);
    glib::timeout_add_local_once(Duration::from_secs(8), move || {
        if generation_cell.get() == generation {
            banner.set_reveal_child(false);
        }
    });
}

fn clear_box(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn section_label(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("title-4");
    label.set_xalign(0.0);
    label
}

fn muted_label(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("dim-label");
    label.set_xalign(0.0);
    label.set_margin_top(4);
    label.set_margin_bottom(4);
    label
}

fn icon_button(icon: &str, tooltip: &str) -> Button {
    let button = Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build();
    button.add_css_class("flat");
    button
}

fn hero_icon(snapshot: &ManagerSnapshot, devices: &[NetworkDevice]) -> &'static str {
    if snapshot.state == ManagerState::Connecting {
        return "network-wireless-acquiring-symbolic";
    }
    if devices
        .iter()
        .any(|device| device.kind == DeviceKind::Wifi && device.state == DeviceState::Connected)
    {
        "network-wireless-symbolic"
    } else if devices
        .iter()
        .any(|device| device.state == DeviceState::Connected)
    {
        "network-wired-symbolic"
    } else {
        "network-offline-symbolic"
    }
}

fn device_icon(device: &NetworkDevice) -> &'static str {
    match device.kind {
        DeviceKind::Wifi => match device.state {
            DeviceState::Connected => "network-wireless-symbolic",
            DeviceState::Connecting | DeviceState::IpConfig => {
                "network-wireless-acquiring-symbolic"
            }
            _ => "network-wireless-offline-symbolic",
        },
        DeviceKind::Ethernet => {
            if device.state == DeviceState::Connected {
                "network-wired-symbolic"
            } else {
                "network-wired-disconnected-symbolic"
            }
        }
        DeviceKind::Loopback => "network-loopback-symbolic",
        DeviceKind::Other => "network-transmit-receive-symbolic",
    }
}

fn signal_icon(quality: u8) -> &'static str {
    match quality {
        0..=25 => "network-wireless-signal-weak-symbolic",
        26..=50 => "network-wireless-signal-ok-symbolic",
        51..=75 => "network-wireless-signal-good-symbolic",
        _ => "network-wireless-signal-excellent-symbolic",
    }
}

fn device_name(device: &NetworkDevice) -> String {
    match device.kind {
        DeviceKind::Wifi => device
            .ssid
            .as_ref()
            .map_or_else(|| format!("Wi-Fi ({})", device.ifname), |ssid| ssid.clone()),
        _ => device.ifname.clone(),
    }
}

fn device_description(device: &NetworkDevice) -> String {
    match device.state {
        DeviceState::Connected => {
            if let Some(ip) = device.primary_ipv4() {
                format!("已连接 · {ip}")
            } else {
                "已连接".to_owned()
            }
        }
        DeviceState::Failed => device
            .last_error
            .clone()
            .unwrap_or_else(|| "连接失败".to_owned()),
        _ => device.state.label().to_owned(),
    }
}

fn device_meta(device: &NetworkDevice) -> String {
    let mut parts = vec![device.kind.label().to_owned(), device.ifname.clone()];
    if let Some(mac) = &device.mac {
        parts.push(mac.clone());
    }
    if let Some(mtu) = device.mtu {
        parts.push(format!("MTU {mtu}"));
    }
    parts.join(" · ")
}

fn describe_connection(device: &NetworkDevice) -> String {
    match device.kind {
        DeviceKind::Wifi => format!(
            "{}: {}",
            device.ifname,
            device.ssid.as_deref().unwrap_or("Wi-Fi")
        ),
        _ => format!(
            "{}: {}",
            device.ifname,
            device.primary_ipv4().unwrap_or("无地址")
        ),
    }
}

fn install_css() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
