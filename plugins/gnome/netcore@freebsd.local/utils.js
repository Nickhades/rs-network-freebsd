// Pure utility helpers for the netcore GNOME Shell extension.
//
// This module has no GNOME Shell dependencies so it can be exercised with
// plain gjs.  Labels mirror the Rust model in crates/netcore-common so the
// shell shows exactly the same wording as the daemon and GUI.

export const SECURITY_LABELS = Object.freeze({
    open: '开放',
    wep: 'WEP',
    wpa_psk: 'WPA',
    wpa2_psk: 'WPA2',
    wpa3_sae: 'WPA3',
    enterprise: '企业级',
    unknown: '未知',
});

const DEVICE_KIND_LABELS = Object.freeze({
    wifi: 'Wi-Fi',
    ethernet: '以太网',
    loopback: '回环',
    other: '其他',
});

const DEVICE_STATE_LABELS = Object.freeze({
    unknown: '未知',
    unmanaged: '未托管',
    unavailable: '不可用',
    disconnected: '未连接',
    connecting: '正在连接',
    ip_config: '正在获取地址',
    connected: '已连接',
    disconnecting: '正在断开',
    failed: '连接失败',
});

const MANAGER_STATE_LABELS = Object.freeze({
    offline: '离线',
    connecting: '正在连接',
    connected: '已连接',
    disconnecting: '正在断开',
});

const CONNECTIVITY_LABELS = Object.freeze({
    none: '无连接',
    local: '本地网络',
    unknown: '未知',
});

export function asArray(value) {
    return Array.isArray(value) ? value : [];
}

export function asString(value, fallback = '') {
    return typeof value === 'string' ? value : fallback;
}

export function securityLabel(security) {
    return SECURITY_LABELS[security] ?? SECURITY_LABELS.unknown;
}

/**
 * Mirror of netcore_common::model::Security::requires_password().
 * "unknown" is treated like open by the daemon protocol, so the extension
 * does the same instead of inventing its own policy.
 */
export function requiresPassword(security) {
    return security !== 'open' && security !== 'unknown';
}

/**
 * Convert dBm to 0..100 exactly like WifiNetwork::quality_percent().
 */
export function qualityPercent(signal) {
    const value = Number(signal);
    const dbm = Number.isFinite(value) ? value : -100;
    const clamped = Math.max(-100, Math.min(-50, dbm));
    return Math.round(Math.max(0, Math.min(100, (clamped + 100) * 2)));
}

export function signalIcon(signal) {
    const quality = qualityPercent(signal);
    if (quality >= 80)
        return 'network-wireless-signal-excellent-symbolic';
    if (quality >= 60)
        return 'network-wireless-signal-good-symbolic';
    if (quality >= 40)
        return 'network-wireless-signal-ok-symbolic';
    if (quality >= 20)
        return 'network-wireless-signal-weak-symbolic';
    return 'network-wireless-signal-none-symbolic';
}

export function deviceKindLabel(kind) {
    return DEVICE_KIND_LABELS[kind] ?? DEVICE_KIND_LABELS.other;
}

export function deviceStateLabel(state) {
    return DEVICE_STATE_LABELS[state] ?? String(state ?? '未知');
}

export function managerStateLabel(state) {
    return MANAGER_STATE_LABELS[state] ?? String(state ?? '未知');
}

export function connectivityLabel(connectivity) {
    return CONNECTIVITY_LABELS[connectivity] ?? String(connectivity ?? '未知');
}

export function connectedNetwork(device) {
    const networks = asArray(device?.networks);
    return networks.find(network => network?.connected) ??
        networks.find(network => network?.ssid && network.ssid === device?.ssid) ??
        null;
}

export function deviceDisplayName(device) {
    if (!device)
        return '';
    if (device.kind === 'wifi' && typeof device.ssid === 'string' && device.ssid.length > 0)
        return device.ssid;
    return asString(device.ifname, '网络设备');
}

export function primaryIpv4(device) {
    const addresses = asArray(device?.ipv4);
    if (addresses.length === 0)
        return '';
    const address = asString(addresses[0]);
    const slash = address.indexOf('/');
    return slash >= 0 ? address.slice(0, slash) : address;
}

export function deviceIcon(device) {
    if (!device)
        return 'network-offline-symbolic';

    if (device.kind === 'wifi') {
        const network = connectedNetwork(device);
        return network ? signalIcon(network.signal) : 'network-wireless-symbolic';
    }
    if (device.kind === 'ethernet')
        return 'network-wired-symbolic';
    return 'network-transmit-receive-symbolic';
}

export function truncate(text, maxLength) {
    const value = String(text ?? '');
    if (value.length <= maxLength)
        return value;
    return value.slice(0, Math.max(0, maxLength - 1)) + '…';
}

export function oneLine(text) {
    return String(text ?? '').replace(/\s+/g, ' ').trim();
}

/**
 * Check basic Wi-Fi password constraints before sending a request to the
 * daemon.  The daemon remains authoritative; this only catches empty or
 * obviously too-short WPA passphrases in the UI.
 */
export function passwordProblem(security, password) {
    if (typeof password !== 'string' || password.length === 0)
        return '请输入密码';
    if (['wpa_psk', 'wpa2_psk', 'wpa3_sae'].includes(security)) {
        const isHexKey = /^[0-9a-fA-F]{64}$/.test(password);
        if (password.length < 8 && !isHexKey)
            return 'WPA 密码至少需要 8 个字符';
    }
    return '';
}
