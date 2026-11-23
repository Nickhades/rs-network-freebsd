/*
    SPDX-FileCopyrightText: 2026 netcore contributors
    SPDX-License-Identifier: BSD-2-Clause
*/

#pragma once

#include <QDBusMessage>
#include <QHash>
#include <QObject>
#include <QString>
#include <QVariantList>
#include <QVariantMap>

#include <functional>

class QDBusServiceWatcher;
class QTimer;

/*!
 * Asynchronous bridge between the netcore daemon (system bus) and QML.
 *
 * All D-Bus traffic runs on the GUI thread but never blocks it: requests use
 * QDBusConnection::asyncCall() and replies are delivered through
 * QDBusPendingCallWatcher. QDBusInterface is deliberately not used because its
 * constructor performs a synchronous Introspect call; a missing or slow daemon
 * would freeze plasmashell.
 */
class NetcoreBackend : public QObject
{
    Q_OBJECT

    Q_PROPERTY(QVariantMap snapshot READ snapshot NOTIFY snapshotChanged)
    Q_PROPERTY(QVariantList devices READ devices NOTIFY snapshotChanged)
    Q_PROPERTY(QVariantList wifiDevices READ wifiDevices NOTIFY snapshotChanged)
    Q_PROPERTY(QVariantList wifiNetworks READ wifiNetworks NOTIFY snapshotChanged)
    Q_PROPERTY(QString wifiDevice READ wifiDevice WRITE setWifiDevice NOTIFY wifiDeviceChanged)
    Q_PROPERTY(bool hasWifiDevice READ hasWifiDevice NOTIFY snapshotChanged)

    Q_PROPERTY(bool daemonAvailable READ daemonAvailable NOTIFY daemonAvailableChanged)
    Q_PROPERTY(bool busy READ busy NOTIFY busyChanged)
    Q_PROPERTY(QString lastError READ lastError NOTIFY lastErrorChanged)

    Q_PROPERTY(QString state READ state NOTIFY snapshotChanged)
    Q_PROPERTY(QString stateLabel READ stateLabel NOTIFY snapshotChanged)
    Q_PROPERTY(QString connectivityLabel READ connectivityLabel NOTIFY snapshotChanged)
    Q_PROPERTY(QString activeDevice READ activeDevice NOTIFY snapshotChanged)
    Q_PROPERTY(QString activeKind READ activeKind NOTIFY snapshotChanged)
    Q_PROPERTY(QString activeName READ activeName NOTIFY snapshotChanged)
    Q_PROPERTY(QString activeSsid READ activeSsid NOTIFY snapshotChanged)
    Q_PROPERTY(QString activeIpv4 READ activeIpv4 NOTIFY snapshotChanged)
    Q_PROPERTY(QString hostname READ hostname NOTIFY snapshotChanged)
    Q_PROPERTY(QString daemonVersion READ daemonVersion NOTIFY snapshotChanged)
    Q_PROPERTY(QString wpaBackend READ wpaBackend NOTIFY snapshotChanged)
    Q_PROPERTY(bool netlinkActive READ netlinkActive NOTIFY snapshotChanged)
    Q_PROPERTY(bool wifiEnabled READ wifiEnabled NOTIFY snapshotChanged)
    Q_PROPERTY(bool wifiAvailable READ wifiAvailable NOTIFY snapshotChanged)

    Q_PROPERTY(QString compactIcon READ compactIcon NOTIFY summaryChanged)
    Q_PROPERTY(QString toolTipTitle READ toolTipTitle NOTIFY summaryChanged)
    Q_PROPERTY(QString toolTipSubText READ toolTipSubText NOTIFY summaryChanged)

public:
    explicit NetcoreBackend(QObject *parent = nullptr);
    ~NetcoreBackend() override;

    [[nodiscard]] QVariantMap snapshot() const;
    [[nodiscard]] QVariantList devices() const;
    [[nodiscard]] QVariantList wifiDevices() const;
    [[nodiscard]] QVariantList wifiNetworks() const;
    [[nodiscard]] QString wifiDevice() const;
    [[nodiscard]] bool hasWifiDevice() const;

    [[nodiscard]] bool daemonAvailable() const;
    [[nodiscard]] bool busy() const;
    [[nodiscard]] QString lastError() const;

    [[nodiscard]] QString state() const;
    [[nodiscard]] QString stateLabel() const;
    [[nodiscard]] QString connectivityLabel() const;
    [[nodiscard]] QString activeDevice() const;
    [[nodiscard]] QString activeKind() const;
    [[nodiscard]] QString activeName() const;
    [[nodiscard]] QString activeSsid() const;
    [[nodiscard]] QString activeIpv4() const;
    [[nodiscard]] QString hostname() const;
    [[nodiscard]] QString daemonVersion() const;
    [[nodiscard]] QString wpaBackend() const;
    [[nodiscard]] bool netlinkActive() const;
    [[nodiscard]] bool wifiEnabled() const;
    [[nodiscard]] bool wifiAvailable() const;

    [[nodiscard]] QString compactIcon() const;
    [[nodiscard]] QString toolTipTitle() const;
    [[nodiscard]] QString toolTipSubText() const;

    // User actions exposed to QML. Every call is asynchronous.
    Q_INVOKABLE void refresh();
    Q_INVOKABLE void scan();
    Q_INVOKABLE void connectWifi(const QString &ifname,
                                 const QString &ssid,
                                 const QString &password,
                                 bool hidden);
    Q_INVOKABLE void disconnectDevice(const QString &ifname);
    Q_INVOKABLE void setDeviceEnabled(const QString &ifname, bool enabled);
    Q_INVOKABLE void setWifiEnabled(bool enabled);
    Q_INVOKABLE void openGui();
    Q_INVOKABLE void clearError();
    Q_INVOKABLE void setWifiDevice(const QString &ifname);

Q_SIGNALS:
    void snapshotChanged();
    void wifiDeviceChanged();
    void daemonAvailableChanged();
    void busyChanged();
    void lastErrorChanged();
    void summaryChanged();

private Q_SLOTS:
    void onSnapshotChanged(const QString &json);
    void onNetworkScanFinished(const QString &ifname, const QString &networksJson);
    void onDaemonError(const QString &code, const QString &message);

private:
    using Completion = std::function<void(const QDBusMessage &reply, bool success)>;

    void callAsync(const QString &method,
                   const QVariantList &arguments,
                   int timeoutMs,
                   bool blockingOperation,
                   const QString &errorContext,
                   Completion completion = {});
    void applySnapshot(const QString &json);
    void applyScanResult(const QString &json);
    void updateWifiSelection(const QString &preferredIfname = QString());
    void resetSnapshotData();
    void setDaemonAvailable(bool available);
    void setBusy(bool busy);
    void setLastError(const QString &message, bool transportError = false);
    [[nodiscard]] int connectedWifiQuality() const;

    QVariantMap m_snapshot;
    QVariantList m_devices;
    QVariantList m_wifiDevices;
    QVariantList m_wifiNetworks;
    QHash<QString, QVariantList> m_networksByDevice;
    QString m_wifiDevice;

    QDBusServiceWatcher *m_serviceWatcher = nullptr;
    QTimer *m_refreshTimer = nullptr;

    bool m_daemonAvailable = false;
    bool m_busy = false;
    bool m_refreshPending = false;
    int m_pendingOperations = 0;
    QString m_lastError;
    bool m_lastErrorIsTransport = false;
};
