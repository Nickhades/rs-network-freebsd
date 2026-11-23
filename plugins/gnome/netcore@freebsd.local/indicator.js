// Native GNOME Shell panel indicator for the netcore network manager.
//
// The indicator is a pure D-Bus frontend: it never shells out for state or
// actions.  All menu contents are rebuilt from ManagerSnapshot JSON documents
// received over Gio.DBusConnection (see netcoreClient.js).

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as ModalDialog from 'resource:///org/gnome/shell/ui/modalDialog.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

import {
    compareNetworks,
    createDeviceItem,
    createWifiItem,
    deviceSectionSignature,
    wifiSectionSignature,
} from './menuItems.js';
import {NetcoreClient} from './netcoreClient.js';
import {PanelTooltip} from './panelTooltip.js';
import {
    asArray,
    asString,
    connectedNetwork,
    connectivityLabel,
    deviceDisplayName,
    passwordProblem,
    primaryIpv4,
    qualityPercent,
    requiresPassword,
    securityLabel,
    signalIcon,
    truncate,
    oneLine,
} from './utils.js';

const NOTICE_TIMEOUT_SECONDS = 20;
const STATUS_MAX = 80;
const DETAIL_MAX = 110;
const NOTICE_MAX = 140;
const SSID_MAX = 48;

export const NetcoreIndicator = GObject.registerClass(
class NetcoreIndicator extends PanelMenu.Button {
    _init() {
        super._init(0.0, 'netcore 网络管理器', false);

        this._destroyed = false;
        this._snapshot = null;
        this._unavailable = false;
        this._daemonMessage = '';
        this._busy = false;
        this._scanning = false;
        this._pendingText = '';
        this._noticeText = '';
        this._noticeError = false;
        this._noticeSourceId = 0;
        this._wifiTogglePending = false;
        this._updatingWifiSwitch = false;
        this._wifiSectionSignature = null;
        this._deviceSectionSignature = null;
        this._dialog = null;

        this._icon = new St.Icon({
            style_class: 'system-status-icon netcore-panel-icon',
            icon_name: 'network-offline-symbolic',
        });
        this.add_child(this._icon);

        this._buildMenu();

        this._tooltip = new PanelTooltip(this);
        this._tooltip.setText('netcore：正在连接…');

        this.menu.connect('open-state-changed', (_menu, open) => {
            if (!open)
                return;
            this._tooltip?.hide();
            this._client?.refresh();
        });

        this._setStatus('正在连接 netcore…', '正在读取网络状态…');

        this._client = new NetcoreClient({
            onSnapshot: snapshot => this._onSnapshot(snapshot),
            onUnavailable: message => this._onUnavailable(message),
        });

        try {
            this._client.start();
        } catch (error) {
            this._onUnavailable(oneLine(error?.message || '无法启动 netcore 客户端'));
        }
    }

    // ------------------------------------------------------------- menu setup

    _buildMenu() {
        this._statusItem = new PopupMenu.PopupMenuItem('', {
            reactive: false,
            can_focus: false,
        });
        this._statusItem.remove_style_class_name('popup-inactive-menu-item');
        this._statusItem.label.add_style_class_name('netcore-status-title');
        this.menu.addMenuItem(this._statusItem);

        this._detailItem = new PopupMenu.PopupMenuItem('', {
            reactive: false,
            can_focus: false,
        });
        this._detailItem.label.add_style_class_name('netcore-status-detail');
        this.menu.addMenuItem(this._detailItem);

        this._noticeItem = new PopupMenu.PopupMenuItem('', {
            reactive: false,
            can_focus: false,
        });
        this._noticeItem.remove_style_class_name('popup-inactive-menu-item');
        this._noticeItem.label.add_style_class_name('netcore-notice');
        this._noticeItem.visible = false;
        this.menu.addMenuItem(this._noticeItem);

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        this._wifiSwitch = new PopupMenu.PopupSwitchMenuItem('Wi-Fi', false);
        this._wifiSwitch.connect('toggled', (_item, state) => this._onWifiToggled(state));
        this._wifiSwitch.setSensitive(false);
        this.menu.addMenuItem(this._wifiSwitch);

        this._scanItem = new PopupMenu.PopupImageMenuItem(
            '扫描 Wi-Fi 网络', 'view-refresh-symbolic');
        this._scanItem.connect('activate', () => this._scan());
        this._scanItem.setSensitive(false);
        this.menu.addMenuItem(this._scanItem);

        this._wifiSection = new PopupMenu.PopupMenuSection();
        this.menu.addMenuItem(this._wifiSection);

        this._deviceSection = new PopupMenu.PopupMenuSection();
        this.menu.addMenuItem(this._deviceSection);

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        this._guiItem = new PopupMenu.PopupImageMenuItem(
            '打开网络管理器', 'preferences-system-symbolic');
        this._guiItem.connect('activate', () => this._launchGui());
        this.menu.addMenuItem(this._guiItem);
    }

    // ---------------------------------------------------------- state plumbing

    _onSnapshot(snapshot) {
        if (this._destroyed || !snapshot)
            return;
        this._snapshot = snapshot;
        this._unavailable = false;
        this._daemonMessage = '';
        this._renderAll();
    }

    _onUnavailable(message) {
        if (this._destroyed)
            return;

        this._unavailable = true;
        this._daemonMessage = oneLine(message) || 'netcore 守护进程不可用';
        this._snapshot = null;
        this._busy = false;
        this._scanning = false;
        this._pendingText = '';
        this._wifiTogglePending = false;
        this._clearNotice();
        this._closeDialog();
        this._renderAll();
    }

    _activeDevice() {
        const snapshot = this._snapshot;
        if (!snapshot)
            return null;

        const devices = asArray(snapshot.devices)
            .filter(device => device && device.kind !== 'loopback');

        if (snapshot.active_device) {
            const named = devices.find(device => device.ifname === snapshot.active_device);
            if (named)
                return named;
        }

        return devices.find(device => device.state === 'connected') ??
            devices.find(device => device.state === 'ip_config') ??
            devices.find(device => device.state === 'connecting') ??
            null;
    }

    _wifiDevice() {
        const snapshot = this._snapshot;
        if (!snapshot)
            return null;

        const devices = asArray(snapshot.devices)
            .filter(device => device && device.kind === 'wifi');

        if (devices.length === 0)
            return null;

        const active = devices.find(device => device.ifname === snapshot.active_device);
        if (active)
            return active;

        return devices.find(device => typeof device.ssid === 'string' && device.ssid.length > 0) ??
            devices.find(device => asArray(device.networks).length > 0) ??
            devices[0];
    }

    // ---------------------------------------------------------------- rendering

    _renderAll() {
        if (this._destroyed)
            return;
        try {
            this._updatePanel();
            this._updateStatus();
            this._updateWifiSwitch();
            this._updateScanItem();
            this._rebuildWifiSectionIfNeeded();
            this._rebuildDeviceSectionIfNeeded();
            this._renderNotice();
        } catch (error) {
            console.error('[netcore] failed to render menu:', error);
        }
    }

    _updatePanel() {
        let icon = 'network-offline-symbolic';
        let tooltip = 'netcore：正在连接…';

        if (this._unavailable) {
            icon = 'network-error-symbolic';
            tooltip = `netcore：${this._daemonMessage || '守护进程不可用'}`;
        } else if (!this._snapshot) {
            icon = 'network-offline-symbolic';
            tooltip = 'netcore：正在连接…';
        } else {
            const snapshot = this._snapshot;
            const active = this._activeDevice();

            switch (snapshot.state) {
            case 'connected': {
                if (active) {
                    if (active.kind === 'wifi') {
                        const network = connectedNetwork(active);
                        icon = network
                            ? signalIcon(network.signal)
                            : 'network-wireless-symbolic';
                    } else if (active.kind === 'ethernet') {
                        icon = 'network-wired-symbolic';
                    } else {
                        icon = 'network-transmit-receive-symbolic';
                    }
                } else if (snapshot.wifi_available) {
                    icon = 'network-wireless-symbolic';
                } else {
                    icon = 'network-wired-symbolic';
                }

                const name = active ? deviceDisplayName(active) : asString(snapshot.hostname);
                const ip = active ? primaryIpv4(active) : '';
                tooltip = `netcore：已连接${name ? ` · ${name}` : ''}${ip ? ` · ${ip}` : ''}`;
                break;
            }
            case 'connecting':
                icon = 'network-wireless-acquiring-symbolic';
                tooltip = 'netcore：正在连接…';
                break;
            case 'disconnecting':
                icon = 'network-wireless-acquiring-symbolic';
                tooltip = 'netcore：正在断开…';
                break;
            case 'offline':
            default:
                if (snapshot.wifi_available && !snapshot.wifi_enabled) {
                    icon = 'network-wireless-disabled-symbolic';
                    tooltip = 'netcore：离线（Wi-Fi 已关闭）';
                } else {
                    icon = 'network-offline-symbolic';
                    tooltip = 'netcore：离线';
                }
                break;
            }
        }

        this._icon.icon_name = icon;
        tooltip = truncate(oneLine(tooltip), 120);
        this._tooltip?.setText(tooltip);
        this.accessible_name = tooltip;
    }

    _updateStatus() {
        if (this._unavailable) {
            this._setStatus('netcore 守护进程不可用',
                this._daemonMessage || '请确认 netcore-daemon 正在运行');
            return;
        }

        if (!this._snapshot) {
            this._setStatus('正在连接 netcore…', '正在读取网络状态…');
            return;
        }

        const snapshot = this._snapshot;
        const active = this._activeDevice();
        let status = '未知状态';
        let detail = '';

        switch (snapshot.state) {
        case 'connected': {
            status = snapshot.connectivity === 'local'
                ? `已连接 · ${connectivityLabel(snapshot.connectivity)}`
                : '已连接';
            if (active) {
                const name = deviceDisplayName(active);
                const ip = primaryIpv4(active);
                detail = `${name}${ip ? ` · ${ip}` : ''}`;
            } else {
                detail = '已连接到网络';
            }
            break;
        }
        case 'connecting':
            status = '正在连接…';
            detail = active ? `正在连接 ${deviceDisplayName(active)}…` : '等待网络配置…';
            break;
        case 'disconnecting':
            status = '正在断开…';
            detail = active ? `正在断开 ${deviceDisplayName(active)}…` : '正在释放网络地址…';
            break;
        case 'offline':
            status = '离线';
            if (!snapshot.wifi_available)
                detail = '未检测到 Wi-Fi 设备';
            else if (!snapshot.wifi_enabled)
                detail = 'Wi-Fi 已关闭';
            else
                detail = '未连接到网络';
            break;
        default:
            status = '未知状态';
            detail = 'netcore 返回了无法识别的网络状态';
            break;
        }

        this._setStatus(status, detail);
    }

    _setStatus(status, detail) {
        this._statusItem.label.text = truncate(oneLine(status), STATUS_MAX);
        this._detailItem.label.text = truncate(oneLine(detail), DETAIL_MAX);
        this._detailItem.visible = Boolean(this._detailItem.label.text);
    }

    _updateWifiSwitch() {
        const snapshot = this._snapshot;
        const enabled = snapshot ? Boolean(snapshot.wifi_enabled) : false;

        // Do not fight the user's optimistic state while SetWifiEnabled is
        // still in flight; the callback will either confirm or revert it.
        if (!this._wifiTogglePending) {
            this._updatingWifiSwitch = true;
            try {
                this._wifiSwitch.setToggleState(enabled);
            } finally {
                this._updatingWifiSwitch = false;
            }
        }

        const sensitive = !this._unavailable &&
            Boolean(snapshot?.wifi_available) &&
            !this._busy;
        this._wifiSwitch.setSensitive(sensitive);
    }

    _updateScanItem() {
        const snapshot = this._snapshot;
        const device = this._wifiDevice();
        const canScan = !this._unavailable &&
            Boolean(snapshot) &&
            Boolean(device) &&
            Boolean(snapshot?.wifi_enabled) &&
            device?.enabled !== false &&
            !this._busy;

        this._scanItem.label.text = this._scanning ? '正在扫描…' : '扫描 Wi-Fi 网络';
        this._scanItem.setSensitive(canScan && !this._scanning);
    }

    _addPlaceholder(section, text) {
        const item = new PopupMenu.PopupMenuItem(text, {
            reactive: false,
            can_focus: false,
        });
        item.label.add_style_class_name('netcore-placeholder');
        section.addMenuItem(item);
    }

    _rebuildWifiSectionIfNeeded() {
        const signature = this._wifiSignature();
        if (signature === this._wifiSectionSignature)
            return;

        this._wifiSectionSignature = signature;
        this._wifiSection.removeAll();

        if (this._unavailable || !this._snapshot)
            return;

        const snapshot = this._snapshot;
        this._wifiSection.addMenuItem(
            new PopupMenu.PopupSeparatorMenuItem('Wi-Fi 网络'));

        if (!snapshot.wifi_available) {
            this._addPlaceholder(this._wifiSection, '未检测到 Wi-Fi 设备');
            return;
        }
        if (!snapshot.wifi_enabled) {
            this._addPlaceholder(this._wifiSection, 'Wi-Fi 已关闭，可在上方重新开启');
            return;
        }

        const device = this._wifiDevice();
        if (!device) {
            this._addPlaceholder(this._wifiSection, '未检测到 Wi-Fi 设备');
            return;
        }

        const networks = asArray(device.networks).slice().sort(compareNetworks);
        if (networks.length === 0) {
            const text = snapshot.state === 'connecting'
                ? '正在获取网络列表…'
                : '未发现网络，点击“扫描 Wi-Fi 网络”刷新';
            this._addPlaceholder(this._wifiSection, text);
            return;
        }

        for (const network of networks) {
            this._wifiSection.addMenuItem(createWifiItem(device, network,
                (ifname, activated) => this._onNetworkActivated(ifname, activated)));
        }
    }

    _wifiSignature() {
        if (this._unavailable || !this._snapshot)
            return 'hidden';
        return wifiSectionSignature(this._snapshot, this._wifiDevice());
    }

    _rebuildDeviceSectionIfNeeded() {
        const signature = this._deviceSignature();
        if (signature === this._deviceSectionSignature)
            return;

        this._deviceSectionSignature = signature;
        this._deviceSection.removeAll();

        if (this._unavailable || !this._snapshot)
            return;

        const snapshot = this._snapshot;
        this._deviceSection.addMenuItem(
            new PopupMenu.PopupSeparatorMenuItem('设备'));

        const devices = asArray(snapshot.devices)
            .filter(device => device && device.kind !== 'loopback');

        if (devices.length === 0) {
            this._addPlaceholder(this._deviceSection, '未发现可管理的网络设备');
            return;
        }

        for (const device of devices) {
            this._deviceSection.addMenuItem(createDeviceItem(device,
                this._deviceAction(device),
                (itemDevice, type) => this._onDeviceAction(itemDevice, type)));
        }
    }

    _deviceSignature() {
        if (this._unavailable || !this._snapshot)
            return 'hidden';
        return deviceSectionSignature(this._snapshot);
    }

    _deviceAction(device) {
        if (!device?.managed || !device.ifname)
            return null;

        switch (device.state) {
        case 'connected':
        case 'connecting':
        case 'ip_config':
            return {type: 'disconnect', label: '断开'};
        case 'disconnected':
        case 'failed':
        case 'unknown':
        case 'unavailable':
            return {type: 'connect', label: '连接'};
        default:
            return null;
        }
    }

    // --------------------------------------------------------------- actions

    _beginAction(message) {
        this._busy = true;
        this._pendingText = message;
        this._clearNotice();
        this._renderAll();
    }

    _finishAction() {
        this._busy = false;
        this._pendingText = '';
    }

    _onWifiToggled(state) {
        if (this._destroyed || this._updatingWifiSwitch)
            return;

        if (this._unavailable || !this._snapshot || this._busy || !this._client) {
            this._updateWifiSwitch();
            return;
        }

        this._wifiTogglePending = true;
        this._beginAction(state ? '正在开启 Wi-Fi…' : '正在关闭 Wi-Fi…');

        this._client.setWifiEnabled(state, error => {
            if (this._destroyed)
                return;

            this._wifiTogglePending = false;
            this._finishAction();

            if (error) {
                if (!this._unavailable) {
                    this._setNotice(
                        `无法${state ? '开启' : '关闭'} Wi-Fi：` +
                        oneLine(error?.message || '未知错误'), true);
                }
            } else {
                // Optimistically reflect the new setting until the daemon
                // publishes the authoritative snapshot.
                if (this._snapshot)
                    this._snapshot.wifi_enabled = state;
                this._setNotice(state ? 'Wi-Fi 已开启' : 'Wi-Fi 已关闭', false);
                this._client?.refresh();
            }

            this._renderAll();
        });
    }

    _scan() {
        if (this._destroyed || this._busy || this._scanning || !this._client)
            return;

        const device = this._wifiDevice();
        if (!device || this._unavailable || !this._snapshot)
            return;

        const ifname = asString(device.ifname);
        if (!ifname)
            return;

        this._scanning = true;
        this._beginAction(`正在扫描 ${ifname}…`);

        this._client.scanWifi(ifname, (error, result) => {
            if (this._destroyed)
                return;

            this._scanning = false;
            this._finishAction();

            if (error) {
                if (!this._unavailable) {
                    this._setNotice(
                        `扫描失败：${oneLine(error?.message || '未知错误')}`, true);
                }
            } else {
                const networks = Array.isArray(result?.networks) ? result.networks : [];
                this._mergeScanResult(result);
                this._setNotice(`扫描完成，发现 ${networks.length} 个网络`, false);
                this._client?.refresh();
            }

            this._renderAll();
        });
    }

    _mergeScanResult(result) {
        if (!this._snapshot || !result || !Array.isArray(result.networks))
            return;

        const ifname = asString(result.ifname);
        const device = asArray(this._snapshot.devices)
            .find(candidate => candidate?.ifname === ifname);
        if (device)
            device.networks = result.networks;
    }

    _onNetworkActivated(ifname, network) {
        if (this._destroyed || this._busy || !this._snapshot)
            return;

        const device = this._wifiDevice();
        const alreadyConnected = Boolean(network?.connected) ||
            (asString(device?.ssid) !== '' &&
                asString(network?.ssid) === asString(device?.ssid));
        if (alreadyConnected)
            return;

        const ssid = asString(network?.ssid);
        if (!ssid)
            return;

        const actualIfname = asString(ifname);
        if (!actualIfname)
            return;

        if (requiresPassword(asString(network?.security, 'unknown')))
            this._openPasswordDialog(actualIfname, network);
        else
            this._connectWifi(actualIfname, ssid, '');
    }

    _connectWifi(ifname, ssid, password) {
        if (this._destroyed || this._busy || !this._client)
            return;

        this._beginAction(`正在连接 ${ssid}…`);

        this._client.connectWifi(ifname, ssid, password, false, error => {
            if (this._destroyed)
                return;

            this._finishAction();

            if (error) {
                if (!this._unavailable) {
                    this._setNotice(
                        `连接 ${ssid} 失败：${oneLine(error?.message || '未知错误')}`,
                        true);
                }
            } else {
                this._setNotice(`已连接到 ${ssid}`, false);
                this._client?.refresh();
            }

            this._renderAll();
        });
    }

    _onDeviceAction(device, type) {
        if (this._destroyed || this._busy || !this._client)
            return;

        const ifname = asString(device?.ifname);
        if (!ifname)
            return;

        const disconnect = type === 'disconnect';
        this._beginAction(disconnect
            ? `正在断开 ${ifname}…`
            : `正在启用 ${ifname}…`);

        const done = (error, result) => {
            if (this._destroyed)
                return;

            this._finishAction();

            if (error) {
                if (!this._unavailable) {
                    this._setNotice(
                        `操作失败：${oneLine(error?.message || '未知错误')}`, true);
                }
            } else {
                const fallback = disconnect ? `${ifname} 已断开` : `${ifname} 已启用`;
                const message = oneLine(result?.message) || fallback;
                this._setNotice(message, false);
                this._client?.refresh();
            }

            this._renderAll();
        };

        if (disconnect)
            this._client.disconnectDevice(ifname, done);
        else
            this._client.setDeviceEnabled(ifname, true, done);
    }

    _launchGui() {
        try {
            const launcher = new Gio.SubprocessLauncher({
                flags: Gio.SubprocessFlags.STDOUT_SILENCE |
                    Gio.SubprocessFlags.STDERR_SILENCE,
            });
            const process = launcher.spawnv(['netcore-gui']);

            // The GUI is independent of the shell; reaping its exit status
            // keeps GLib from leaving a zombie without blocking the UI.
            process.wait_async(null, (source, res) => {
                try {
                    source.wait_finish(res);
                } catch {
                    // The process already failed; spawnv() reported hard errors.
                }
            });

            this._setNotice('已启动 netcore-gui', false);
        } catch (error) {
            this._setNotice(
                `无法启动 netcore-gui：${oneLine(error?.message || '未知错误')}`,
                true);
        }
    }

    // ----------------------------------------------------------- password dialog

    _openPasswordDialog(ifname, network) {
        if (this._destroyed || this._dialog)
            return;

        const ssid = asString(network?.ssid);
        if (!ssid)
            return;

        const security = asString(network?.security, 'unknown');
        const quality = qualityPercent(network?.signal);

        const dialog = new ModalDialog.ModalDialog({
            styleClass: 'netcore-password-dialog',
            destroyOnClose: true,
        });
        this._dialog = dialog;

        const box = new St.BoxLayout({
            vertical: true,
            spacing: 8,
            style_class: 'netcore-password-box',
        });
        const title = new St.Label({
            text: `连接到 “${truncate(ssid, SSID_MAX)}”`,
            style_class: 'netcore-dialog-title',
        });
        const meta = new St.Label({
            text: `${securityLabel(security)} · 信号 ${quality}%`,
            style_class: 'netcore-dialog-meta dim-label',
        });
        const entry = new St.PasswordEntry({
            hint_text: 'Wi-Fi 密码',
            show_peek_icon: true,
            can_focus: true,
            x_expand: true,
            style_class: 'netcore-password-entry',
        });
        const errorLabel = new St.Label({
            text: '',
            style_class: 'netcore-dialog-error',
            visible: false,
        });
        errorLabel.clutter_text.line_wrap = true;

        box.add_child(title);
        box.add_child(meta);
        box.add_child(entry);
        box.add_child(errorLabel);
        dialog.contentLayout.add_child(box);

        let submitted = false;
        const submit = () => {
            if (submitted || this._destroyed)
                return;

            const password = entry.get_text();
            const problem = passwordProblem(security, password);
            if (problem) {
                errorLabel.text = problem;
                errorLabel.show();
                entry.grab_key_focus();
                return;
            }

            submitted = true;
            this._dialog = null;
            dialog.close(global.get_current_time());
            this._connectWifi(ifname, ssid, password);
        };

        dialog.setButtons([
            {
                label: '取消',
                action: () => dialog.close(global.get_current_time()),
                key: Clutter.KEY_Escape,
            },
            {
                label: '连接',
                action: submit,
                default: true,
            },
        ]);
        entry.clutter_text.connect('activate', submit);
        dialog.connect('closed', () => {
            if (this._dialog === dialog)
                this._dialog = null;
        });
        dialog.setInitialKeyFocus(entry);

        let opened = false;
        try {
            opened = dialog.open(global.get_current_time());
        } catch (error) {
            console.error('[netcore] failed to open password dialog:', error);
        }

        if (!opened) {
            this._dialog = null;
            try {
                dialog.destroy();
            } catch {
                // Already destroyed while refusing to open.
            }
            this._setNotice('无法打开密码对话框，请关闭其他模态窗口后重试', true);
        }
    }

    _closeDialog() {
        const dialog = this._dialog;
        if (!dialog)
            return;
        this._dialog = null;
        try {
            dialog.close(global.get_current_time());
        } catch {
            try {
                dialog.destroy();
            } catch {
                // The dialog is already gone.
            }
        }
    }

    // ------------------------------------------------------------------ notices

    _setNotice(text, isError) {
        if (this._destroyed)
            return;

        if (this._noticeSourceId) {
            GLib.source_remove(this._noticeSourceId);
            this._noticeSourceId = 0;
        }

        this._noticeText = truncate(oneLine(text), NOTICE_MAX);
        this._noticeError = Boolean(isError);

        if (!this._noticeText) {
            this._renderNotice();
            return;
        }

        this._noticeSourceId = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT, NOTICE_TIMEOUT_SECONDS, () => {
                this._noticeSourceId = 0;
                this._noticeText = '';
                this._noticeError = false;
                if (!this._destroyed)
                    this._renderNotice();
                return GLib.SOURCE_REMOVE;
            });

        this._renderNotice();
    }

    _clearNotice() {
        if (this._noticeSourceId) {
            GLib.source_remove(this._noticeSourceId);
            this._noticeSourceId = 0;
        }
        this._noticeText = '';
        this._noticeError = false;
    }

    _renderNotice() {
        const text = this._pendingText || this._noticeText;
        this._noticeItem.visible = Boolean(text);
        if (!text)
            return;

        this._noticeItem.label.text = truncate(text, NOTICE_MAX);
        this._noticeItem.label.remove_style_class_name('netcore-notice-error');
        if (this._noticeError && !this._pendingText)
            this._noticeItem.label.add_style_class_name('netcore-notice-error');
    }

    // ----------------------------------------------------------------- cleanup

    _onDestroy() {
        this._destroyed = true;

        this._clearNotice();
        this._closeDialog();

        if (this._client) {
            this._client.stop();
            this._client = null;
        }

        if (this._tooltip) {
            this._tooltip.destroy();
            this._tooltip = null;
        }

        super._onDestroy();
    }
});