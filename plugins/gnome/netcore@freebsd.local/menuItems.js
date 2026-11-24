// Creation helpers for the dynamic netcore popup rows.
//
// Keeping these small, pure row builders out of indicator.js makes the state
// machine easier to follow; they take plain snapshot objects and an activation
// callback, and never touch D-Bus.

import Clutter from 'gi://Clutter';
import St from 'gi://St';

import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

import {
    asArray,
    asString,
    deviceIcon,
    deviceKindLabel,
    deviceStateLabel,
    oneLine,
    primaryIpv4,
    qualityPercent,
    securityLabel,
    signalIcon,
    truncate,
} from './utils.js';

const SSID_MAX = 48;
const DEVICE_META_MAX = 90;

function signalValue(network) {
    const value = Number(network?.signal);
    return Number.isFinite(value) ? value : -100;
}

export function compareNetworks(a, b) {
    const diff = signalValue(b) - signalValue(a);
    if (diff !== 0)
        return diff;
    return asString(a?.ssid).localeCompare(asString(b?.ssid));
}

/** Signature that changes only when the visible Wi-Fi list must be rebuilt. */
export function wifiSectionSignature(snapshot, device) {
    if (!snapshot)
        return 'hidden';

    const networks = device
        ? asArray(device.networks).map(network => [
            asString(network?.ssid),
            signalValue(network),
            asString(network?.security),
            Boolean(network?.connected),
        ]).sort((a, b) => a[0].localeCompare(b[0]))
        : [];

    return JSON.stringify([
        asString(snapshot.state),
        Boolean(snapshot.wifi_available),
        Boolean(snapshot.wifi_enabled),
        asString(device?.ifname),
        asString(device?.ssid),
        device?.enabled !== false,
        networks,
    ]);
}

/** Signature that changes only when the visible device rows must be rebuilt. */
export function deviceSectionSignature(snapshot) {
    if (!snapshot)
        return 'hidden';

    const devices = asArray(snapshot.devices)
        .filter(device => device && device.kind !== 'loopback')
        .map(device => [
            asString(device.ifname),
            asString(device.kind),
            asString(device.state),
            Boolean(device.managed),
            Boolean(device.enabled),
            Boolean(device.carrier),
            asArray(device.ipv4).join(','),
            asString(device.ssid),
            asString(device.last_error),
        ]);

    return JSON.stringify([asString(snapshot.active_device), devices]);
}

export function createWifiItem(device, network, onActivate) {
    const ifname = asString(device?.ifname);
    const ssid = asString(network?.ssid) || '（隐藏网络）';
    const security = asString(network?.security, 'unknown');
    const connected = Boolean(network?.connected) ||
        (asString(device?.ssid) !== '' &&
            asString(network?.ssid) === asString(device?.ssid));
    const quality = qualityPercent(network?.signal);

    const item = new PopupMenu.PopupBaseMenuItem({
        style_class: 'netcore-network-item',
    });

    item.add_child(new St.Icon({
        icon_name: signalIcon(network?.signal),
        style_class: 'popup-menu-icon netcore-network-icon',
        y_align: Clutter.ActorAlign.CENTER,
    }));

    const labels = new St.BoxLayout({
        vertical: true,
        x_expand: true,
    });
    labels.add_child(new St.Label({
        text: truncate(ssid, SSID_MAX),
        x_expand: true,
        style_class: 'netcore-network-title',
    }));
    labels.add_child(new St.Label({
        text: `${securityLabel(security)} · 信号 ${quality}%`,
        x_expand: true,
        style_class: 'netcore-network-meta dim-label',
    }));
    item.add_child(labels);

    if (connected) {
        item.add_child(new St.Icon({
            icon_name: 'object-select-symbolic',
            style_class: 'popup-menu-icon netcore-check-icon',
            y_align: Clutter.ActorAlign.CENTER,
        }));
    }

    item.accessible_name = `${ssid}，${securityLabel(security)}，信号质量 ${quality}%` +
        `${connected ? '，已连接' : ''}`;
    item.connect('activate', () => onActivate(ifname, network));
    return item;
}

export function createDeviceItem(device, action, onActivate) {
    const item = new PopupMenu.PopupBaseMenuItem({
        reactive: Boolean(action),
        activate: Boolean(action),
        hover: Boolean(action),
        can_focus: Boolean(action),
        style_class: 'netcore-device-item',
    });

    item.add_child(new St.Icon({
        icon_name: deviceIcon(device),
        style_class: 'popup-menu-icon netcore-device-icon',
        y_align: Clutter.ActorAlign.CENTER,
    }));

    const labels = new St.BoxLayout({
        vertical: true,
        x_expand: true,
    });
    labels.add_child(new St.Label({
        text: truncate(asString(device?.ifname, '未知设备'), 32),
        x_expand: true,
        style_class: 'netcore-device-title',
    }));

    const metaText = deviceMeta(device);
    labels.add_child(new St.Label({
        text: truncate(metaText, DEVICE_META_MAX),
        x_expand: true,
        style_class: 'netcore-device-meta dim-label',
    }));
    item.add_child(labels);

    if (action) {
        item.add_child(new St.Label({
            text: action.label,
            style_class: 'netcore-action-label',
            y_align: Clutter.ActorAlign.CENTER,
        }));
        item.connect('activate', () => onActivate(device, action.type));
    }

    item.accessible_name = `${asString(device?.ifname)}，${metaText}` +
        `${action ? `，${action.label}` : ''}`;
    return item;
}

function deviceMeta(device) {
    const parts = [
        deviceKindLabel(device?.kind),
        deviceStateLabel(device?.state),
    ];
    if (device?.kind === 'wifi' && typeof device.ssid === 'string' && device.ssid)
        parts.push(device.ssid);
    const ip = primaryIpv4(device);
    if (ip)
        parts.push(ip);
    if (device?.enabled === false)
        parts.push('已禁用');
    if (device?.last_error)
        parts.push(oneLine(device.last_error));
    return parts.join(' · ');
}