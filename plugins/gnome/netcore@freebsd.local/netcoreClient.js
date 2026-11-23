// Async D-Bus client for org.netcore.NetworkManager.
//
// The shell main loop must never block: every remote call goes through
// Gio.DBusConnection.call() with a completion callback, all calls share one
// Gio.Cancellable, and stop() cancels/unsubscribes everything.  SnapshotChanged
// is the primary update path; a five second poll is kept as a fallback because
// signals can be missed across daemon restarts.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

export const NETCORE_SERVICE = 'org.netcore.NetworkManager';
export const NETCORE_PATH = '/org/netcore/NetworkManager';
export const NETCORE_INTERFACE = 'org.netcore.NetworkManager';

const GET_SNAPSHOT_TIMEOUT = 5000;
const SCAN_TIMEOUT = 30000;
const CONNECT_TIMEOUT = 60000;
const DEVICE_ACTION_TIMEOUT = 20000;
const WIFI_TOGGLE_TIMEOUT = 20000;
const POLL_INTERVAL_SECONDS = 5;

const STRING_REPLY_TYPE = new GLib.VariantType('(s)');
const NO_ARGUMENTS = new GLib.Variant('()', []);

function parseJson(text) {
    if (typeof text !== 'string' || text.length === 0)
        return null;
    try {
        return JSON.parse(text);
    } catch {
        return null;
    }
}

function replyString(reply) {
    if (!reply)
        return null;
    try {
        const unpacked = reply.deepUnpack();
        return typeof unpacked[0] === 'string' ? unpacked[0] : null;
    } catch {
        return null;
    }
}

function parseReplyJson(reply) {
    return parseJson(replyString(reply));
}

function errorText(error) {
    if (error === null || error === undefined)
        return '未知错误';
    if (typeof error === 'string')
        return error;
    return String(error.message ?? error);
}

/**
 * Turn a GLib/GDBus error into text suitable for the menu plus a flag telling
 * the caller whether the daemon itself should now be considered unavailable.
 */
function classifyError(error) {
    const raw = errorText(error);
    const match = raw.match(/^GDBus\.Error:([A-Za-z0-9_.]+):\s*([\s\S]*)$/);
    const name = match ? match[1] : '';
    const detail = (match ? match[2] : raw).trim();
    const lower = raw.toLowerCase();

    const noService =
        name === 'org.freedesktop.DBus.Error.ServiceUnknown' ||
        name === 'org.freedesktop.DBus.Error.NameHasNoOwner' ||
        name.startsWith('org.freedesktop.DBus.Error.Spawn.') ||
        name === 'org.freedesktop.DBus.Error.Disconnected' ||
        lower.includes('was not provided by any .service files') ||
        lower.includes('could not activate remote peer') ||
        lower.includes('name has no owner');

    if (noService) {
        if (name === 'org.freedesktop.DBus.Error.Disconnected')
            return {message: '系统 D-Bus 连接已断开', unavailable: true};
        if (name.startsWith('org.freedesktop.DBus.Error.Spawn.') ||
            lower.includes('could not activate remote peer'))
            return {message: '无法启动 netcore 守护进程', unavailable: true};
        return {message: 'netcore 守护进程未运行', unavailable: true};
    }

    // The daemon promises human-readable messages for its own error domain;
    // show those unchanged instead of inventing a translation layer.
    if (name.startsWith(`${NETCORE_INTERFACE}.Error.`))
        return {message: detail || 'netcore 操作失败', unavailable: false};

    if (/NotAuthorized|AccessDenied|AuthFailed/.test(name) ||
        lower.includes('not authorized')) {
        return {message: '权限不足或认证被取消', unavailable: false};
    }

    if (name === 'org.freedesktop.DBus.Error.Timeout' ||
        name === 'org.freedesktop.DBus.Error.NoReply' ||
        lower.includes('timeout was reached') ||
        lower.includes('did not receive a reply')) {
        return {message: 'netcore 响应超时，请稍后重试', unavailable: false};
    }

    return {message: detail || name || '操作失败', unavailable: false};
}

export class NetcoreClient {
    /**
     * @param {object} handlers - {onSnapshot(snapshot), onUnavailable(message)}
     * @param {object} [options] - {busType, busAddress}; defaults to the system
     *   bus.  The overrides exist only so the client can be tested against a
     *   private bus.
     */
    constructor(handlers = {}, options = {}) {
        this._handlers = handlers;
        this._busType = options.busType ?? Gio.BusType.SYSTEM;
        this._busAddress = options.busAddress ?? null;

        this._connection = null;
        this._cancellable = null;
        this._pollSourceId = 0;
        this._snapshotSubId = 0;
        this._ownerSubId = 0;

        this._started = false;
        this._stopped = false;
        this._busPending = false;
        this._refreshPending = false;
        this._refreshQueued = false;
        this._daemonAvailable = false;
        this._unavailableMessage = '';
    }

    get daemonAvailable() {
        return this._daemonAvailable;
    }

    get busLabel() {
        if (this._busAddress)
            return '测试 D-Bus';
        return this._busType === Gio.BusType.SESSION ? '会话 D-Bus' : '系统 D-Bus';
    }

    start() {
        if (this._started || this._stopped)
            return;
        this._started = true;
        this._cancellable = new Gio.Cancellable();

        this._pollSourceId = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT, POLL_INTERVAL_SECONDS, () => {
                if (this._stopped)
                    return GLib.SOURCE_REMOVE;
                this._poll();
                return GLib.SOURCE_CONTINUE;
            });
        GLib.Source.set_name_by_id(this._pollSourceId, '[netcore] snapshot poll');

        this._ensureBus();
    }

    stop() {
        if (this._stopped)
            return;
        this._stopped = true;

        if (this._pollSourceId) {
            GLib.source_remove(this._pollSourceId);
            this._pollSourceId = 0;
        }

        this._unsubscribeSignals();

        if (this._cancellable) {
            this._cancellable.cancel();
            this._cancellable = null;
        }

        this._connection = null;
        this._handlers = null;
    }

    refresh() {
        if (this._stopped)
            return;
        if (!this._connection) {
            this._ensureBus();
            return;
        }
        if (this._refreshPending) {
            this._refreshQueued = true;
            return;
        }

        this._refreshPending = true;
        this._call('GetSnapshot', null, STRING_REPLY_TYPE, GET_SNAPSHOT_TIMEOUT,
            (info, reply) => {
                this._refreshPending = false;
                if (this._stopped)
                    return;

                if (info) {
                    if (info.unavailable)
                        this._markUnavailable(info.message);
                    else if (!this._daemonAvailable)
                        this._markUnavailable(`无法获取网络状态：${info.message}`);
                    else
                        console.debug(`[netcore] GetSnapshot failed: ${info.message}`);
                } else {
                    const snapshot = parseReplyJson(reply);
                    if (snapshot && Array.isArray(snapshot.devices)) {
                        this._daemonAvailable = true;
                        this._unavailableMessage = '';
                        this._safeInvoke(this._handlers?.onSnapshot, snapshot);
                    } else {
                        this._markUnavailable('netcore 返回了无法解析的网络状态');
                    }
                }

                if (this._refreshQueued) {
                    this._refreshQueued = false;
                    this.refresh();
                }
            });
    }

    scanWifi(ifname, callback) {
        const parameters = new GLib.Variant('(s)', [String(ifname)]);
        this._call('ScanWifi', parameters, STRING_REPLY_TYPE, SCAN_TIMEOUT,
            (info, reply) => this._safeInvoke(callback, info,
                info ? null : parseReplyJson(reply)));
    }

    connectWifi(ifname, ssid, password, hidden, callback) {
        const parameters = new GLib.Variant('(sssb)', [
            String(ifname),
            String(ssid),
            String(password ?? ''),
            Boolean(hidden),
        ]);
        this._call('ConnectWifi', parameters, STRING_REPLY_TYPE, CONNECT_TIMEOUT,
            (info, reply) => this._safeInvoke(callback, info,
                info ? null : parseReplyJson(reply)));
    }

    disconnectDevice(ifname, callback) {
        const parameters = new GLib.Variant('(s)', [String(ifname)]);
        this._call('DisconnectDevice', parameters, STRING_REPLY_TYPE,
            DEVICE_ACTION_TIMEOUT,
            (info, reply) => this._safeInvoke(callback, info,
                info ? null : parseReplyJson(reply)));
    }

    setDeviceEnabled(ifname, enabled, callback) {
        const parameters = new GLib.Variant('(sb)', [String(ifname), Boolean(enabled)]);
        this._call('SetDeviceEnabled', parameters, STRING_REPLY_TYPE,
            DEVICE_ACTION_TIMEOUT,
            (info, reply) => this._safeInvoke(callback, info,
                info ? null : parseReplyJson(reply)));
    }

    setWifiEnabled(enabled, callback) {
        const parameters = new GLib.Variant('(b)', [Boolean(enabled)]);
        this._call('SetWifiEnabled', parameters, null, WIFI_TOGGLE_TIMEOUT,
            (info) => this._safeInvoke(callback, info, null));
    }

    // ---------------------------------------------------------------- private

    _poll() {
        if (this._stopped)
            return;
        if (!this._connection) {
            this._ensureBus();
            return;
        }
        this.refresh();
    }

    _ensureBus() {
        if (this._stopped || this._connection || this._busPending)
            return;

        this._busPending = true;
        const finishBus = (source, res) => {
            this._busPending = false;
            if (this._stopped)
                return;

            let connection;
            try {
                connection = this._busAddress
                    ? Gio.DBusConnection.new_for_address_finish(res)
                    : Gio.bus_get_finish(res);
            } catch (error) {
                if (this._stopped)
                    return;
                const info = classifyError(error);
                this._markUnavailable(`无法连接${this.busLabel}：${info.message}`);
                return;
            }

            this._connection = connection;
            this._subscribeSignals();
            this.refresh();
        };

        try {
            if (this._busAddress) {
                // Only used by integration tests / development; the shipped
                // extension always uses the system bus.
                Gio.DBusConnection.new_for_address(
                    this._busAddress,
                    Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT |
                        Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
                    null,
                    this._cancellable,
                    finishBus);
            } else {
                Gio.bus_get(this._busType, this._cancellable, finishBus);
            }
        } catch (error) {
            this._busPending = false;
            this._markUnavailable(`无法连接${this.busLabel}：${errorText(error)}`);
        }
    }

    _subscribeSignals() {
        if (!this._connection)
            return;

        if (!this._snapshotSubId) {
            try {
                this._snapshotSubId = this._connection.signal_subscribe(
                    NETCORE_SERVICE,
                    NETCORE_INTERFACE,
                    'SnapshotChanged',
                    NETCORE_PATH,
                    null,
                    Gio.DBusSignalFlags.NONE,
                    this._onSnapshotSignal.bind(this));
            } catch (error) {
                console.warn(`[netcore] cannot subscribe SnapshotChanged: ${errorText(error)}`);
            }
        }

        if (!this._ownerSubId) {
            try {
                this._ownerSubId = this._connection.signal_subscribe(
                    'org.freedesktop.DBus',
                    'org.freedesktop.DBus',
                    'NameOwnerChanged',
                    '/org/freedesktop/DBus',
                    NETCORE_SERVICE,
                    Gio.DBusSignalFlags.NONE,
                    this._onNameOwnerChanged.bind(this));
            } catch (error) {
                console.warn(`[netcore] cannot subscribe NameOwnerChanged: ${errorText(error)}`);
            }
        }
    }

    _unsubscribeSignals() {
        const connection = this._connection;
        if (connection) {
            for (const id of [this._snapshotSubId, this._ownerSubId]) {
                if (!id)
                    continue;
                try {
                    connection.signal_unsubscribe(id);
                } catch {
                    // The connection may already be gone; nothing to clean up.
                }
            }
        }
        this._snapshotSubId = 0;
        this._ownerSubId = 0;
    }

    _onSnapshotSignal(connection, sender, objectPath, interfaceName, signalName, parameters) {
        if (this._stopped)
            return;

        let snapshot = null;
        try {
            const [json] = parameters.deepUnpack();
            snapshot = parseJson(json);
        } catch {
            snapshot = null;
        }

        if (!snapshot || !Array.isArray(snapshot.devices)) {
            // Fall back to a full fetch if a malformed signal arrives.
            this.refresh();
            return;
        }

        this._daemonAvailable = true;
        this._unavailableMessage = '';
        this._safeInvoke(this._handlers?.onSnapshot, snapshot);
    }

    _onNameOwnerChanged(connection, sender, objectPath, interfaceName, signalName, parameters) {
        if (this._stopped)
            return;

        let unpacked;
        try {
            unpacked = parameters.deepUnpack();
        } catch {
            return;
        }

        const [name, oldOwner, newOwner] = unpacked;
        if (name !== NETCORE_SERVICE)
            return;

        if (newOwner)
            this.refresh();
        else if (oldOwner)
            this._markUnavailable('netcore 守护进程已停止');
    }

    _markUnavailable(message) {
        const text = message || 'netcore 守护进程不可用';
        this._daemonAvailable = false;
        if (this._unavailableMessage === text)
            return;
        this._unavailableMessage = text;
        this._safeInvoke(this._handlers?.onUnavailable, text);
    }

    _call(method, parameters, replyType, timeout, callback) {
        if (this._stopped) {
            this._safeInvoke(callback,
                {message: 'netcore 客户端已停止', unavailable: true}, null);
            return;
        }

        const connection = this._connection;
        if (!connection) {
            this._safeInvoke(callback,
                {message: 'netcore 守护进程未连接', unavailable: true}, null);
            return;
        }

        try {
            connection.call(
                NETCORE_SERVICE,
                NETCORE_PATH,
                NETCORE_INTERFACE,
                method,
                parameters ?? NO_ARGUMENTS,
                replyType,
                Gio.DBusCallFlags.NONE,
                timeout,
                this._cancellable,
                (source, res) => {
                    if (this._stopped)
                        return;

                    let reply;
                    try {
                        reply = source.call_finish(res);
                    } catch (error) {
                        if (this._stopped)
                            return;
                        const info = classifyError(error);
                        if (info.unavailable)
                            this._markUnavailable(info.message);
                        this._safeInvoke(callback, info, null);
                        return;
                    }

                    this._safeInvoke(callback, null, reply);
                });
        } catch (error) {
            const info = classifyError(error);
            if (info.unavailable)
                this._markUnavailable(info.message);
            this._safeInvoke(callback, info, null);
        }
    }

    _safeInvoke(callback, ...args) {
        if (typeof callback !== 'function')
            return;
        try {
            callback(...args);
        } catch (error) {
            console.error('[netcore] D-Bus callback failed:', error);
        }
    }
}
