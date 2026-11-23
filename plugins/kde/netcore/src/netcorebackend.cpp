/*
    SPDX-FileCopyrightText: 2026 netcore contributors
    SPDX-License-Identifier: BSD-2-Clause
*/

#include "netcorebackend.h"

#include <QDBusConnection>
#include <QDBusPendingCall>
#include <QDBusPendingCallWatcher>
#include <QDBusServiceWatcher>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QProcess>
#include <QStandardPaths>
#include <QStringList>
#include <QTimer>
#include <QtGlobal>

#include <algorithm>
#include <utility>

namespace
{
const QString kService = QStringLiteral("org.netcore.NetworkManager");
const QString kPath = QStringLiteral("/org/netcore/NetworkManager");
const QString kInterface = QStringLiteral("org.netcore.NetworkManager");

// SnapshotChanged is the primary update path; this only papers over lost
// signals or a daemon restart that raced with the service watcher.
constexpr int kRefreshIntervalMs = 15000;
constexpr int kSnapshotTimeoutMs = 15000;
constexpr int kOperationTimeoutMs = 30000;
constexpr int kScanTimeoutMs = 60000;

bool isTransportError(const QString &name)
{
    return name == QLatin1String("org.freedesktop.DBus.Error.ServiceUnknown")
        || name == QLatin1String("org.freedesktop.DBus.Error.NameHasNoOwner")
        || name == QLatin1String("org.freedesktop.DBus.Error.Disconnected")
        || name == QLatin1String("org.freedesktop.DBus.Error.NoServer");
}

QString managerStateLabel(const QString &state)
{
    if (state == QLatin1String("offline")) {
        return QStringLiteral("离线");
    }
    if (state == QLatin1String("connecting")) {
        return QStringLiteral("正在连接");
    }
    if (state == QLatin1String("connected")) {
        return QStringLiteral("已连接");
    }
    if (state == QLatin1String("disconnecting")) {
        return QStringLiteral("正在断开");
    }
    return QStringLiteral("未知");
}

QString connectivityStateLabel(const QString &connectivity)
{
    if (connectivity == QLatin1String("none")) {
        return QStringLiteral("无连接");
    }
    if (connectivity == QLatin1String("local")) {
        return QStringLiteral("本地网络");
    }
    return QStringLiteral("未知");
}

QString deviceKindLabel(const QString &kind)
{
    if (kind == QLatin1String("wifi")) {
        return QStringLiteral("Wi-Fi");
    }
    if (kind == QLatin1String("ethernet")) {
        return QStringLiteral("以太网");
    }
    if (kind == QLatin1String("loopback")) {
        return QStringLiteral("回环");
    }
    return QStringLiteral("其他");
}

QString deviceStateLabel(const QString &state)
{
    if (state == QLatin1String("unknown")) {
        return QStringLiteral("未知");
    }
    if (state == QLatin1String("unmanaged")) {
        return QStringLiteral("未托管");
    }
    if (state == QLatin1String("unavailable")) {
        return QStringLiteral("不可用");
    }
    if (state == QLatin1String("disconnected")) {
        return QStringLiteral("未连接");
    }
    if (state == QLatin1String("connecting")) {
        return QStringLiteral("正在连接");
    }
    if (state == QLatin1String("ip_config")) {
        return QStringLiteral("正在获取地址");
    }
    if (state == QLatin1String("connected")) {
        return QStringLiteral("已连接");
    }
    if (state == QLatin1String("disconnecting")) {
        return QStringLiteral("正在断开");
    }
    if (state == QLatin1String("failed")) {
        return QStringLiteral("连接失败");
    }
    return QStringLiteral("未知");
}

QString securityLabel(const QString &security)
{
    if (security == QLatin1String("open")) {
        return QStringLiteral("开放");
    }
    if (security == QLatin1String("wep")) {
        return QStringLiteral("WEP");
    }
    if (security == QLatin1String("wpa_psk")) {
        return QStringLiteral("WPA");
    }
    if (security == QLatin1String("wpa2_psk")) {
        return QStringLiteral("WPA2");
    }
    if (security == QLatin1String("wpa3_sae")) {
        return QStringLiteral("WPA3");
    }
    if (security == QLatin1String("enterprise")) {
        return QStringLiteral("企业级");
    }
    return QStringLiteral("未知");
}

// Mirrors netcore_common::model::Security::requires_password().
bool securityRequiresPassword(const QString &security)
{
    return security != QLatin1String("open") && security != QLatin1String("unknown");
}

int qualityForSignal(int signal)
{
    const int clamped = qBound(-100, signal, -50);
    return qBound(0, (clamped + 100) * 2, 100);
}

QString strengthIcon(int quality, bool connected)
{
    const int bucket = quality >= 80 ? 100 : quality >= 60 ? 75 : quality >= 35 ? 50 : quality >= 15 ? 25 : 0;
    if (connected) {
        return QStringLiteral("network-wireless-connected-%1-symbolic")
            .arg(bucket, 2, 10, QLatin1Char('0'));
    }

    const char *name = quality >= 80 ? "excellent"
        : quality >= 60            ? "good"
        : quality >= 35            ? "ok"
        : quality >= 15            ? "weak"
                                   : "none";
    return QStringLiteral("network-wireless-signal-%1-symbolic").arg(QLatin1String(name));
}

QString bandLabel(int frequency)
{
    if (frequency <= 0) {
        return QString();
    }
    if (frequency >= 5925) {
        return QStringLiteral("6 GHz");
    }
    if (frequency >= 4900) {
        return QStringLiteral("5 GHz");
    }
    return QStringLiteral("2.4 GHz");
}

QString deviceIcon(const QString &kind, bool connected)
{
    if (kind == QLatin1String("wifi")) {
        return QStringLiteral("network-wireless-symbolic");
    }
    if (kind == QLatin1String("ethernet")) {
        return connected ? QStringLiteral("network-wired-symbolic")
                         : QStringLiteral("network-wired-disconnected-symbolic");
    }
    if (kind == QLatin1String("loopback")) {
        return QStringLiteral("network-loopback-symbolic");
    }
    return QStringLiteral("network-transmit-receive-symbolic");
}

QString firstAddress(const QJsonArray &addresses)
{
    if (addresses.isEmpty()) {
        return QString();
    }
    return addresses.first().toString().section(QLatin1Char('/'), 0, 0);
}

QString joinedAddresses(const QJsonArray &addresses)
{
    QStringList values;
    values.reserve(addresses.size());
    for (const QJsonValue &value : addresses) {
        const QString address = value.toString();
        if (!address.isEmpty()) {
            values.append(address);
        }
    }
    return values.join(QStringLiteral(", "));
}

QVariantMap enrichNetwork(const QJsonObject &object)
{
    const QString ssid = object.value(QStringLiteral("ssid")).toString();
    const int signal = object.value(QStringLiteral("signal")).toInt(0);
    const int quality = qualityForSignal(signal);
    const int frequency = object.value(QStringLiteral("frequency")).toInt(0);
    const QString security = object.value(QStringLiteral("security")).toString(QStringLiteral("unknown"));
    const bool connected = object.value(QStringLiteral("connected")).toBool(false);
    const bool requiresPassword = securityRequiresPassword(security);

    QVariantMap network;
    network.insert(QStringLiteral("ssid"), ssid);
    network.insert(QStringLiteral("displaySsid"), ssid.isEmpty() ? QStringLiteral("隐藏网络") : ssid);
    network.insert(QStringLiteral("bssid"), object.value(QStringLiteral("bssid")).toString());
    network.insert(QStringLiteral("signal"), signal);
    network.insert(QStringLiteral("signalLabel"), QStringLiteral("%1 dBm").arg(signal));
    network.insert(QStringLiteral("quality"), quality);
    network.insert(QStringLiteral("frequency"), frequency);
    network.insert(QStringLiteral("bandLabel"), bandLabel(frequency));
    network.insert(QStringLiteral("security"), security);
    network.insert(QStringLiteral("securityLabel"), securityLabel(security));
    network.insert(QStringLiteral("requiresPassword"), requiresPassword);
    network.insert(QStringLiteral("securityIcon"), requiresPassword ? QStringLiteral("lock-symbolic") : QString());
    network.insert(QStringLiteral("connected"), connected);
    network.insert(QStringLiteral("strengthIcon"), strengthIcon(quality, connected));
    return network;
}

QVariantList enrichNetworks(const QJsonArray &array)
{
    QVariantList networks;
    networks.reserve(array.size());
    for (const QJsonValue &value : array) {
        if (!value.isObject()) {
            continue;
        }
        const QJsonObject object = value.toObject();
        // Empty SSIDs are hidden BSS entries; the UI has an explicit
        // "connect to a hidden network" flow for those.
        if (object.value(QStringLiteral("ssid")).toString().isEmpty()) {
            continue;
        }
        networks.append(enrichNetwork(object));
    }

    std::stable_sort(networks.begin(), networks.end(), [](const QVariant &left, const QVariant &right) {
        const QVariantMap leftMap = left.toMap();
        const QVariantMap rightMap = right.toMap();
        const int leftSignal = leftMap.value(QStringLiteral("signal")).toInt();
        const int rightSignal = rightMap.value(QStringLiteral("signal")).toInt();
        if (leftSignal != rightSignal) {
            return leftSignal > rightSignal;
        }
        return QString::compare(leftMap.value(QStringLiteral("ssid")).toString(),
                                rightMap.value(QStringLiteral("ssid")).toString(),
                                Qt::CaseInsensitive)
            < 0;
    });
    return networks;
}

bool hasWifiDeviceNamed(const QVariantList &devices, const QString &ifname)
{
    if (ifname.isEmpty()) {
        return false;
    }
    for (const QVariant &entry : devices) {
        if (entry.toMap().value(QStringLiteral("ifname")).toString() == ifname) {
            return true;
        }
    }
    return false;
}
} // namespace

NetcoreBackend::NetcoreBackend(QObject *parent)
    : QObject(parent)
{
    m_serviceWatcher = new QDBusServiceWatcher(
        kService,
        QDBusConnection::systemBus(),
        QDBusServiceWatcher::WatchForRegistration | QDBusServiceWatcher::WatchForUnregistration,
        this);

    connect(m_serviceWatcher, &QDBusServiceWatcher::serviceRegistered, this, [this](const QString &service) {
        if (service == kService) {
            refresh();
        }
    });
    connect(m_serviceWatcher, &QDBusServiceWatcher::serviceUnregistered, this, [this](const QString &service) {
        if (service != kService) {
            return;
        }
        setDaemonAvailable(false);
        resetSnapshotData();
        setLastError(QStringLiteral("netcore 守护进程已停止"), true);
    });

    QDBusConnection bus = QDBusConnection::systemBus();
    bus.connect(kService,
                kPath,
                kInterface,
                QStringLiteral("SnapshotChanged"),
                this,
                SLOT(onSnapshotChanged(QString)));
    bus.connect(kService,
                kPath,
                kInterface,
                QStringLiteral("NetworkScanFinished"),
                this,
                SLOT(onNetworkScanFinished(QString,QString)));
    bus.connect(kService,
                kPath,
                kInterface,
                QStringLiteral("Error"),
                this,
                SLOT(onDaemonError(QString,QString)));

    m_refreshTimer = new QTimer(this);
    m_refreshTimer->setInterval(kRefreshIntervalMs);
    m_refreshTimer->setTimerType(Qt::VeryCoarseTimer);
    connect(m_refreshTimer, &QTimer::timeout, this, &NetcoreBackend::refresh);
    m_refreshTimer->start();

    // The service watcher only reports future changes, so ask once now.
    refresh();
}

NetcoreBackend::~NetcoreBackend() = default;

QVariantMap NetcoreBackend::snapshot() const
{
    return m_snapshot;
}

QVariantList NetcoreBackend::devices() const
{
    return m_devices;
}

QVariantList NetcoreBackend::wifiDevices() const
{
    return m_wifiDevices;
}

QVariantList NetcoreBackend::wifiNetworks() const
{
    return m_wifiNetworks;
}

QString NetcoreBackend::wifiDevice() const
{
    return m_wifiDevice;
}

bool NetcoreBackend::hasWifiDevice() const
{
    return !m_wifiDevices.isEmpty();
}

bool NetcoreBackend::daemonAvailable() const
{
    return m_daemonAvailable;
}

bool NetcoreBackend::busy() const
{
    return m_busy;
}

QString NetcoreBackend::lastError() const
{
    return m_lastError;
}

QString NetcoreBackend::state() const
{
    return m_snapshot.value(QStringLiteral("state")).toString();
}

QString NetcoreBackend::stateLabel() const
{
    return m_snapshot.value(QStringLiteral("stateLabel")).toString();
}

QString NetcoreBackend::connectivityLabel() const
{
    return m_snapshot.value(QStringLiteral("connectivityLabel")).toString();
}

QString NetcoreBackend::activeDevice() const
{
    return m_snapshot.value(QStringLiteral("activeDevice")).toString();
}

QString NetcoreBackend::activeKind() const
{
    return m_snapshot.value(QStringLiteral("activeKind")).toString();
}

QString NetcoreBackend::activeName() const
{
    return m_snapshot.value(QStringLiteral("activeName")).toString();
}

QString NetcoreBackend::activeSsid() const
{
    return m_snapshot.value(QStringLiteral("activeSsid")).toString();
}

QString NetcoreBackend::activeIpv4() const
{
    return m_snapshot.value(QStringLiteral("activeIpv4")).toString();
}

QString NetcoreBackend::hostname() const
{
    return m_snapshot.value(QStringLiteral("hostname")).toString();
}

QString NetcoreBackend::daemonVersion() const
{
    return m_snapshot.value(QStringLiteral("version")).toString();
}

QString NetcoreBackend::wpaBackend() const
{
    return m_snapshot.value(QStringLiteral("wpaBackend")).toString();
}

bool NetcoreBackend::netlinkActive() const
{
    return m_snapshot.value(QStringLiteral("netlinkActive")).toBool();
}

bool NetcoreBackend::wifiEnabled() const
{
    return m_snapshot.value(QStringLiteral("wifiEnabled")).toBool();
}

bool NetcoreBackend::wifiAvailable() const
{
    return m_snapshot.value(QStringLiteral("wifiAvailable")).toBool();
}

int NetcoreBackend::connectedWifiQuality() const
{
    const QString active = activeDevice();
    const QVariantList activeNetworks = m_networksByDevice.value(active);
    for (const QVariant &entry : activeNetworks) {
        const QVariantMap network = entry.toMap();
        if (network.value(QStringLiteral("connected")).toBool()) {
            return network.value(QStringLiteral("quality")).toInt();
        }
    }
    for (auto it = m_networksByDevice.cbegin(); it != m_networksByDevice.cend(); ++it) {
        for (const QVariant &entry : it.value()) {
            const QVariantMap network = entry.toMap();
            if (network.value(QStringLiteral("connected")).toBool()) {
                return network.value(QStringLiteral("quality")).toInt();
            }
        }
    }
    return 75;
}

QString NetcoreBackend::compactIcon() const
{
    if (!m_daemonAvailable) {
        return QStringLiteral("network-offline-symbolic");
    }

    const QString managerState = state();
    const QString kind = activeKind();
    const bool wifiOn = wifiEnabled();
    const bool wifiPresent = wifiAvailable();

    if (!wifiOn) {
        if (kind == QLatin1String("ethernet") && managerState == QLatin1String("connected")) {
            return QStringLiteral("network-wired-symbolic");
        }
        return QStringLiteral("network-wireless-disconnected-symbolic");
    }

    if (managerState == QLatin1String("connecting")
        || managerState == QLatin1String("disconnecting")) {
        return QStringLiteral("network-wireless-acquiring-symbolic");
    }

    if (kind == QLatin1String("ethernet") && managerState == QLatin1String("connected")) {
        return QStringLiteral("network-wired-symbolic");
    }

    if (managerState == QLatin1String("connected")) {
        return strengthIcon(connectedWifiQuality(), true);
    }

    if (!wifiPresent) {
        return QStringLiteral("network-offline-symbolic");
    }
    return QStringLiteral("network-wireless-signal-none-symbolic");
}

QString NetcoreBackend::toolTipTitle() const
{
    if (!m_daemonAvailable) {
        return QStringLiteral("netcore 未运行");
    }

    if (state() == QLatin1String("connected")) {
        const QString name = activeName();
        if (!name.isEmpty()) {
            if (activeKind() == QLatin1String("wifi")) {
                return QStringLiteral("Wi-Fi：%1").arg(name);
            }
            if (activeKind() == QLatin1String("ethernet")) {
                return QStringLiteral("有线网络：%1").arg(name);
            }
            return name;
        }
        return QStringLiteral("已连接");
    }

    if (!wifiEnabled()) {
        return QStringLiteral("Wi-Fi 已关闭");
    }
    if (!wifiAvailable()) {
        return QStringLiteral("未检测到 Wi-Fi 硬件");
    }
    return QStringLiteral("网络未连接");
}

QString NetcoreBackend::toolTipSubText() const
{
    if (!m_daemonAvailable) {
        return QStringLiteral("无法连接到系统总线上的 netcore 守护进程");
    }

    QStringList parts;
    if (m_busy) {
        parts.append(QStringLiteral("正在操作…"));
    }

    if (state() == QLatin1String("connected")) {
        parts.append(QStringLiteral("已连接"));
        if (!activeIpv4().isEmpty()) {
            parts.append(activeIpv4());
        }
    } else if (state() == QLatin1String("connecting")) {
        parts.append(QStringLiteral("正在连接"));
    } else if (!wifiEnabled()) {
        parts.append(QStringLiteral("打开 Wi-Fi 以扫描网络"));
    } else {
        parts.append(QStringLiteral("点击查看可用网络"));
    }

    if (!hostname().isEmpty()) {
        parts.append(hostname());
    }
    if (!m_lastError.isEmpty() && !m_lastErrorIsTransport) {
        parts.append(m_lastError);
    }
    return parts.join(QStringLiteral(" · "));
}

void NetcoreBackend::refresh()
{
    if (m_refreshPending) {
        return;
    }
    m_refreshPending = true;

    callAsync(QStringLiteral("GetSnapshot"),
              {},
              kSnapshotTimeoutMs,
              false,
              QStringLiteral("读取网络状态失败"),
              [this](const QDBusMessage &reply, bool success) {
                  m_refreshPending = false;
                  if (!success) {
                      return;
                  }
                  applySnapshot(reply.arguments().value(0).toString());
              });
}

void NetcoreBackend::scan()
{
    if (!m_daemonAvailable) {
        setLastError(QStringLiteral("netcore 守护进程未运行"));
        return;
    }
    if (m_wifiDevice.isEmpty()) {
        setLastError(QStringLiteral("未找到可用的 Wi-Fi 设备"));
        return;
    }

    clearError();
    const QString ifname = m_wifiDevice;
    callAsync(QStringLiteral("ScanWifi"),
              {ifname},
              kScanTimeoutMs,
              true,
              QStringLiteral("扫描失败"),
              [this](const QDBusMessage &reply, bool success) {
                  if (!success) {
                      return;
                  }
                  applyScanResult(reply.arguments().value(0).toString());
                  refresh();
              });
}

void NetcoreBackend::connectWifi(const QString &ifname,
                                 const QString &ssid,
                                 const QString &password,
                                 bool hidden)
{
    const QString target = ifname.isEmpty() ? m_wifiDevice : ifname;
    if (!m_daemonAvailable) {
        setLastError(QStringLiteral("netcore 守护进程未运行"));
        return;
    }
    if (target.isEmpty()) {
        setLastError(QStringLiteral("未找到可用的 Wi-Fi 设备"));
        return;
    }
    if (ssid.isEmpty()) {
        setLastError(QStringLiteral("网络名称不能为空"));
        return;
    }

    clearError();
    callAsync(QStringLiteral("ConnectWifi"),
              {target, ssid, password, hidden},
              kOperationTimeoutMs,
              true,
              QStringLiteral("连接失败"),
              [this](const QDBusMessage &reply, bool success) {
                  Q_UNUSED(reply);
                  if (success) {
                      refresh();
                  }
              });
}

void NetcoreBackend::disconnectDevice(const QString &ifname)
{
    if (!m_daemonAvailable) {
        setLastError(QStringLiteral("netcore 守护进程未运行"));
        return;
    }
    if (ifname.isEmpty()) {
        setLastError(QStringLiteral("设备名称不能为空"));
        return;
    }

    clearError();
    callAsync(QStringLiteral("DisconnectDevice"),
              {ifname},
              kOperationTimeoutMs,
              true,
              QStringLiteral("断开连接失败"),
              [this](const QDBusMessage &reply, bool success) {
                  Q_UNUSED(reply);
                  if (success) {
                      refresh();
                  }
              });
}

void NetcoreBackend::setDeviceEnabled(const QString &ifname, bool enabled)
{
    if (!m_daemonAvailable) {
        setLastError(QStringLiteral("netcore 守护进程未运行"));
        return;
    }
    if (ifname.isEmpty()) {
        setLastError(QStringLiteral("设备名称不能为空"));
        return;
    }

    clearError();
    callAsync(QStringLiteral("SetDeviceEnabled"),
              {ifname, enabled},
              kOperationTimeoutMs,
              true,
              QStringLiteral("切换设备状态失败"),
              [this](const QDBusMessage &reply, bool success) {
                  Q_UNUSED(reply);
                  if (success) {
                      refresh();
                  }
              });
}

void NetcoreBackend::setWifiEnabled(bool enabled)
{
    if (!m_daemonAvailable) {
        setLastError(QStringLiteral("netcore 守护进程未运行"));
        return;
    }

    clearError();
    callAsync(QStringLiteral("SetWifiEnabled"),
              {enabled},
              kOperationTimeoutMs,
              true,
              QStringLiteral("切换 Wi-Fi 失败"),
              [this](const QDBusMessage &reply, bool success) {
                  Q_UNUSED(reply);
                  if (success) {
                      refresh();
                  }
              });
}

void NetcoreBackend::openGui()
{
    QString program = QStandardPaths::findExecutable(QStringLiteral("netcore-gui"));
    if (program.isEmpty()) {
        const QString fallback = QStringLiteral("/usr/local/bin/netcore-gui");
        if (QFileInfo::exists(fallback) && QFileInfo(fallback).isExecutable()) {
            program = fallback;
        }
    }
    if (program.isEmpty()) {
        setLastError(QStringLiteral("未找到 netcore-gui，请先安装并将其加入 PATH"));
        return;
    }

    qint64 pid = 0;
    if (!QProcess::startDetached(program, QStringList(), QString(), &pid)) {
        setLastError(QStringLiteral("无法启动 netcore-gui（%1）").arg(program));
    }
}

void NetcoreBackend::clearError()
{
    setLastError(QString(), false);
}

void NetcoreBackend::setWifiDevice(const QString &ifname)
{
    if (ifname == m_wifiDevice) {
        return;
    }
    if (!hasWifiDeviceNamed(m_wifiDevices, ifname)) {
        return;
    }

    m_wifiDevice = ifname;
    m_wifiNetworks = m_networksByDevice.value(ifname);
    Q_EMIT wifiDeviceChanged();
    Q_EMIT snapshotChanged();
}

void NetcoreBackend::onSnapshotChanged(const QString &json)
{
    applySnapshot(json);
}

void NetcoreBackend::onNetworkScanFinished(const QString &ifname, const QString &networksJson)
{
    QJsonParseError error{};
    const QJsonDocument document = QJsonDocument::fromJson(networksJson.toUtf8(), &error);
    if (error.error != QJsonParseError::NoError) {
        setLastError(QStringLiteral("扫描结果解析失败：%1").arg(error.errorString()));
        return;
    }

    const QJsonArray array = document.isArray()
        ? document.array()
        : document.object().value(QStringLiteral("networks")).toArray();
    const QString target = ifname.isEmpty() ? m_wifiDevice : ifname;
    if (target.isEmpty()) {
        return;
    }

    m_networksByDevice.insert(target, enrichNetworks(array));
    if (m_wifiDevice == target || m_wifiDevice.isEmpty()) {
        if (m_wifiDevice.isEmpty()) {
            m_wifiDevice = target;
            Q_EMIT wifiDeviceChanged();
        }
        m_wifiNetworks = m_networksByDevice.value(target);
    }
    setDaemonAvailable(true);
    Q_EMIT snapshotChanged();
    Q_EMIT summaryChanged();
}

void NetcoreBackend::onDaemonError(const QString &code, const QString &message)
{
    setLastError(message.isEmpty() ? QStringLiteral("守护进程报告错误：%1").arg(code) : message);
}

void NetcoreBackend::callAsync(const QString &method,
                               const QVariantList &arguments,
                               int timeoutMs,
                               bool blockingOperation,
                               const QString &errorContext,
                               Completion completion)
{
    if (blockingOperation) {
        ++m_pendingOperations;
        setBusy(true);
    }

    QDBusMessage message = QDBusMessage::createMethodCall(kService, kPath, kInterface, method);
    message.setArguments(arguments);

    QDBusConnection bus = QDBusConnection::systemBus();
    const QDBusPendingCall pending = bus.asyncCall(message, timeoutMs);
    auto *watcher = new QDBusPendingCallWatcher(pending, this);

    connect(watcher,
            &QDBusPendingCallWatcher::finished,
            this,
            [this, watcher, blockingOperation, errorContext, completion = std::move(completion)]() {
                watcher->deleteLater();

                if (blockingOperation) {
                    if (m_pendingOperations > 0) {
                        --m_pendingOperations;
                    }
                    if (m_pendingOperations == 0) {
                        setBusy(false);
                    }
                }

                const QDBusMessage reply = watcher->reply();
                const bool success = reply.type() != QDBusMessage::ErrorMessage;
                if (!success) {
                    const QString errorName = reply.errorName();
                    const QString errorMessage = reply.errorMessage();
                    if (isTransportError(errorName)) {
                        setDaemonAvailable(false);
                        resetSnapshotData();
                        setLastError(QStringLiteral("netcore 守护进程未运行（%1）").arg(errorName), true);
                    } else {
                        setLastError(errorMessage.isEmpty()
                                         ? QStringLiteral("%1：%2").arg(errorContext, errorName)
                                         : QStringLiteral("%1：%2").arg(errorContext, errorMessage));
                    }
                } else {
                    setDaemonAvailable(true);
                    if (m_lastErrorIsTransport) {
                        clearError();
                    }
                }

                if (completion) {
                    completion(reply, success);
                }
            });
}

void NetcoreBackend::applySnapshot(const QString &json)
{
    QJsonParseError error{};
    const QJsonDocument document = QJsonDocument::fromJson(json.toUtf8(), &error);
    if (error.error != QJsonParseError::NoError || !document.isObject()) {
        setLastError(QStringLiteral("网络状态解析失败：%1").arg(error.errorString()));
        return;
    }

    const QJsonObject root = document.object();
    const QJsonArray rawDevices = root.value(QStringLiteral("devices")).toArray();

    // First pass: find the active device (daemon hint, then first connected).
    QString activeDevice = root.value(QStringLiteral("active_device")).toString();
    QJsonObject activeObject;
    for (const QJsonValue &value : rawDevices) {
        if (!value.isObject()) {
            continue;
        }
        const QJsonObject object = value.toObject();
        if (object.value(QStringLiteral("ifname")).toString() == activeDevice) {
            activeObject = object;
            break;
        }
    }
    if (activeObject.isEmpty()) {
        activeDevice.clear();
        for (const QJsonValue &value : rawDevices) {
            if (!value.isObject()) {
                continue;
            }
            const QJsonObject object = value.toObject();
            if (object.value(QStringLiteral("state")).toString() == QLatin1String("connected")) {
                activeObject = object;
                activeDevice = object.value(QStringLiteral("ifname")).toString();
                break;
            }
        }
    }

    QString activeKind;
    QString activeSsid;
    QString activeName;
    QString activeIpv4;
    if (!activeObject.isEmpty()) {
        activeKind = activeObject.value(QStringLiteral("kind")).toString(QStringLiteral("other"));
        activeSsid = activeKind == QLatin1String("wifi")
            ? activeObject.value(QStringLiteral("ssid")).toString()
            : QString();
        activeName = activeKind == QLatin1String("wifi") && !activeSsid.isEmpty()
            ? activeSsid
            : activeDevice;
        activeIpv4 = firstAddress(activeObject.value(QStringLiteral("ipv4")).toArray());
    }

    QVariantList devices;
    QVariantList wifiDevices;
    QHash<QString, QVariantList> networksByDevice;

    for (const QJsonValue &value : rawDevices) {
        if (!value.isObject()) {
            continue;
        }
        const QJsonObject object = value.toObject();
        const QString ifname = object.value(QStringLiteral("ifname")).toString();
        if (ifname.isEmpty()) {
            continue;
        }

        const QString kind = object.value(QStringLiteral("kind")).toString(QStringLiteral("other"));
        const QString deviceState = object.value(QStringLiteral("state")).toString(QStringLiteral("unknown"));
        const QString ssid = object.value(QStringLiteral("ssid")).toString();
        const bool enabled = object.value(QStringLiteral("enabled")).toBool(false);
        const bool managed = object.value(QStringLiteral("managed")).toBool(false);
        const bool connected = deviceState == QLatin1String("connected");
        const bool transitioning = deviceState == QLatin1String("connecting")
            || deviceState == QLatin1String("ip_config")
            || deviceState == QLatin1String("disconnecting");
        const QVariantList networks = enrichNetworks(object.value(QStringLiteral("networks")).toArray());
        networksByDevice.insert(ifname, networks);

        if (kind == QLatin1String("wifi")) {
            QVariantMap wifi;
            wifi.insert(QStringLiteral("ifname"), ifname);
            wifi.insert(QStringLiteral("ssid"), ssid);
            wifi.insert(QStringLiteral("displayName"), ssid.isEmpty() ? ifname : ssid);
            wifi.insert(QStringLiteral("state"), deviceState);
            wifi.insert(QStringLiteral("stateLabel"), deviceStateLabel(deviceState));
            wifi.insert(QStringLiteral("connected"), connected);
            wifi.insert(QStringLiteral("enabled"), enabled);
            wifiDevices.append(wifi);
        }

        if (kind == QLatin1String("loopback")) {
            continue;
        }

        const QString displayName = kind == QLatin1String("wifi") && !ssid.isEmpty() ? ssid : ifname;
        QVariantMap device = object.toVariantMap();
        device.insert(QStringLiteral("kind"), kind);
        device.insert(QStringLiteral("kindLabel"), deviceKindLabel(kind));
        device.insert(QStringLiteral("state"), deviceState);
        device.insert(QStringLiteral("stateLabel"), deviceStateLabel(deviceState));
        device.insert(QStringLiteral("displayName"), displayName);
        device.insert(QStringLiteral("iconName"), deviceIcon(kind, connected));
        device.insert(QStringLiteral("managed"), managed);
        device.insert(QStringLiteral("enabled"), enabled);
        device.insert(QStringLiteral("carrier"), object.value(QStringLiteral("carrier")).toBool(false));
        device.insert(QStringLiteral("connected"), connected);
        device.insert(QStringLiteral("transitioning"), transitioning);
        device.insert(QStringLiteral("active"), ifname == activeDevice);
        device.insert(QStringLiteral("isWifi"), kind == QLatin1String("wifi"));
        device.insert(QStringLiteral("isEthernet"), kind == QLatin1String("ethernet"));
        device.insert(QStringLiteral("primaryIpv4"), firstAddress(object.value(QStringLiteral("ipv4")).toArray()));
        device.insert(QStringLiteral("ipv4Text"), joinedAddresses(object.value(QStringLiteral("ipv4")).toArray()));
        device.insert(QStringLiteral("ipv6Text"), joinedAddresses(object.value(QStringLiteral("ipv6")).toArray()));
        device.insert(QStringLiteral("lastError"), object.value(QStringLiteral("last_error")).toString());
        device.insert(QStringLiteral("networks"), networks);
        device.insert(QStringLiteral("canConnect"), kind == QLatin1String("ethernet") && !connected && managed);
        device.insert(QStringLiteral("canDisconnect"), connected && managed);
        devices.append(device);
    }

    const QString managerState = root.value(QStringLiteral("state")).toString(QStringLiteral("offline"));
    const QString connectivity = root.value(QStringLiteral("connectivity")).toString(QStringLiteral("none"));
    const bool wifiEnabledValue = root.value(QStringLiteral("wifi_enabled")).toBool(false);
    const bool wifiAvailableValue = root.value(QStringLiteral("wifi_available")).toBool(false);

    QVariantMap snapshot = root.toVariantMap();
    snapshot.insert(QStringLiteral("state"), managerState);
    snapshot.insert(QStringLiteral("stateLabel"), managerStateLabel(managerState));
    snapshot.insert(QStringLiteral("connectivity"), connectivity);
    snapshot.insert(QStringLiteral("connectivityLabel"), connectivityStateLabel(connectivity));
    snapshot.insert(QStringLiteral("wifiEnabled"), wifiEnabledValue);
    snapshot.insert(QStringLiteral("wifiAvailable"), wifiAvailableValue);
    snapshot.insert(QStringLiteral("activeDevice"), activeDevice);
    snapshot.insert(QStringLiteral("activeKind"), activeKind);
    snapshot.insert(QStringLiteral("activeName"), activeName);
    snapshot.insert(QStringLiteral("activeSsid"), activeSsid);
    snapshot.insert(QStringLiteral("activeIpv4"), activeIpv4);
    snapshot.insert(QStringLiteral("devices"), devices);
    snapshot.insert(QStringLiteral("wifiDevices"), wifiDevices);
    snapshot.insert(QStringLiteral("hostname"), root.value(QStringLiteral("hostname")).toString());
    snapshot.insert(QStringLiteral("version"), root.value(QStringLiteral("version")).toString());
    snapshot.insert(QStringLiteral("wpaBackend"), root.value(QStringLiteral("wpa_backend")).toString());
    snapshot.insert(QStringLiteral("netlinkActive"), root.value(QStringLiteral("netlink_active")).toBool(false));
    snapshot.insert(QStringLiteral("updatedAt"),
                    static_cast<qulonglong>(root.value(QStringLiteral("updated_at")).toDouble(0.0)));
    snapshot.insert(QStringLiteral("protocolVersion"),
                    root.value(QStringLiteral("protocol_version")).toInt(1));

    m_snapshot = snapshot;
    m_devices = devices;
    m_wifiDevices = wifiDevices;
    m_networksByDevice = networksByDevice;
    updateWifiSelection(activeDevice);

    setDaemonAvailable(true);
    if (m_lastErrorIsTransport) {
        clearError();
    }
    Q_EMIT snapshotChanged();
    Q_EMIT summaryChanged();
}

void NetcoreBackend::applyScanResult(const QString &json)
{
    QJsonParseError error{};
    const QJsonDocument document = QJsonDocument::fromJson(json.toUtf8(), &error);
    if (error.error != QJsonParseError::NoError || !document.isObject()) {
        setLastError(QStringLiteral("扫描结果解析失败：%1").arg(error.errorString()));
        return;
    }

    const QJsonObject object = document.object();
    const QString ifname = object.value(QStringLiteral("ifname")).toString(m_wifiDevice);
    if (ifname.isEmpty()) {
        return;
    }

    const QVariantList networks = enrichNetworks(object.value(QStringLiteral("networks")).toArray());
    m_networksByDevice.insert(ifname, networks);
    if (m_wifiDevice == ifname || m_wifiDevice.isEmpty()) {
        if (m_wifiDevice.isEmpty()) {
            m_wifiDevice = ifname;
            Q_EMIT wifiDeviceChanged();
        }
        m_wifiNetworks = networks;
    }
    Q_EMIT snapshotChanged();
    Q_EMIT summaryChanged();
}

void NetcoreBackend::updateWifiSelection(const QString &preferredIfname)
{
    QString selected = m_wifiDevice;
    if (!hasWifiDeviceNamed(m_wifiDevices, selected)) {
        selected.clear();
    }
    if (selected.isEmpty() && hasWifiDeviceNamed(m_wifiDevices, preferredIfname)) {
        selected = preferredIfname;
    }
    if (selected.isEmpty() && !m_wifiDevices.isEmpty()) {
        selected = m_wifiDevices.first().toMap().value(QStringLiteral("ifname")).toString();
    }

    const bool changed = selected != m_wifiDevice;
    m_wifiDevice = selected;
    m_wifiNetworks = m_networksByDevice.value(selected);
    if (changed) {
        Q_EMIT wifiDeviceChanged();
    }
}

void NetcoreBackend::resetSnapshotData()
{
    const bool hadData = !m_snapshot.isEmpty()
        || !m_devices.isEmpty()
        || !m_wifiDevices.isEmpty()
        || !m_wifiNetworks.isEmpty()
        || !m_wifiDevice.isEmpty();

    m_snapshot.clear();
    m_devices.clear();
    m_wifiDevices.clear();
    m_wifiNetworks.clear();
    m_networksByDevice.clear();

    if (!m_wifiDevice.isEmpty()) {
        m_wifiDevice.clear();
        Q_EMIT wifiDeviceChanged();
    }
    if (hadData) {
        Q_EMIT snapshotChanged();
        Q_EMIT summaryChanged();
    }
}

void NetcoreBackend::setDaemonAvailable(bool available)
{
    if (m_daemonAvailable == available) {
        return;
    }
    m_daemonAvailable = available;
    Q_EMIT daemonAvailableChanged();
    Q_EMIT summaryChanged();
}

void NetcoreBackend::setBusy(bool busy)
{
    if (m_busy == busy) {
        return;
    }
    m_busy = busy;
    Q_EMIT busyChanged();
    Q_EMIT summaryChanged();
}

void NetcoreBackend::setLastError(const QString &message, bool transportError)
{
    if (m_lastError == message && m_lastErrorIsTransport == transportError) {
        return;
    }
    m_lastError = message;
    m_lastErrorIsTransport = transportError;
    Q_EMIT lastErrorChanged();
    Q_EMIT summaryChanged();
}
