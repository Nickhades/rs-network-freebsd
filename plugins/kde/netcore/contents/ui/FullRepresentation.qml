/*
    SPDX-FileCopyrightText: 2026 netcore contributors
    SPDX-License-Identifier: BSD-2-Clause
*/

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.plasma.extras as PlasmaExtras
import org.kde.kirigami as Kirigami
import Netcore.Backend 1.0

/*!
    Full representation: status, Wi-Fi scan/connect, and per-device controls.
*/
PlasmaExtras.Representation {
    id: fullRoot

    required property NetcoreBackend backend

    Layout.minimumWidth: Kirigami.Units.gridUnit * 21
    Layout.preferredWidth: Kirigami.Units.gridUnit * 24
    Layout.minimumHeight: Kirigami.Units.gridUnit * 20
    Layout.preferredHeight: Kirigami.Units.gridUnit * 30

    function activateNetwork(network) {
        if (!network || network.connected) {
            return;
        }
        if (network.requiresPassword) {
            passwordDialog.openFor(network);
        } else {
            backend.connectWifi(backend.wifiDevice, network.ssid, "", false);
        }
    }

    header: PlasmaExtras.PlasmoidHeading {
        contentItem: RowLayout {
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Icon {
                source: fullRoot.backend.compactIcon
                implicitWidth: Kirigami.Units.iconSizes.smallMedium
                implicitHeight: Kirigami.Units.iconSizes.smallMedium
            }

            PlasmaExtras.Heading {
                level: 2
                text: "网络"
                elide: Text.ElideRight
                Layout.fillWidth: true
            }

            PlasmaComponents3.Switch {
                id: wifiSwitch
                text: "Wi-Fi"
                checked: fullRoot.backend.wifiEnabled
                enabled: fullRoot.backend.daemonAvailable
                    && fullRoot.backend.wifiAvailable
                    && !fullRoot.backend.busy
                onToggled: {
                    if (checked !== fullRoot.backend.wifiEnabled) {
                        fullRoot.backend.setWifiEnabled(checked);
                    }
                }
                PlasmaComponents3.ToolTip {
                    text: wifiSwitch.checked ? "关闭 Wi-Fi" : "打开 Wi-Fi"
                }
            }

            PlasmaComponents3.ToolButton {
                icon.name: "view-refresh-symbolic"
                enabled: fullRoot.backend.daemonAvailable && !fullRoot.backend.busy
                onClicked: fullRoot.backend.refresh()
                PlasmaComponents3.ToolTip {
                    text: "刷新网络状态"
                }
            }

            PlasmaComponents3.ToolButton {
                icon.name: "preferences-system-network-symbolic"
                onClicked: fullRoot.backend.openGui()
                PlasmaComponents3.ToolTip {
                    text: "打开 netcore-gui"
                }
            }
        }
    }

    contentItem: Item {
        PlasmaComponents3.ScrollView {
            id: scrollView
            anchors.fill: parent
            visible: fullRoot.backend.daemonAvailable
            contentWidth: availableWidth

            ColumnLayout {
                id: contentColumn

                width: scrollView.contentWidth - Kirigami.Units.smallSpacing * 2
                x: Kirigami.Units.smallSpacing
                y: Kirigami.Units.smallSpacing
                spacing: Kirigami.Units.smallSpacing

                // Overall daemon/connection status.
                PlasmaComponents3.Frame {
                    Layout.fillWidth: true
                    contentItem: RowLayout {
                        spacing: Kirigami.Units.smallSpacing

                        Kirigami.Icon {
                            source: fullRoot.backend.compactIcon
                            implicitWidth: Kirigami.Units.iconSizes.medium
                            implicitHeight: Kirigami.Units.iconSizes.medium
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0

                            PlasmaComponents3.Label {
                                Layout.fillWidth: true
                                font.bold: true
                                elide: Text.ElideRight
                                text: {
                                    const b = fullRoot.backend;
                                    if (b.state === "connected") {
                                        if (b.activeKind === "wifi" && b.activeName) {
                                            return "Wi-Fi：" + b.activeName;
                                        }
                                        if (b.activeKind === "ethernet" && b.activeName) {
                                            return "有线网络：" + b.activeName;
                                        }
                                    }
                                    return b.stateLabel || "离线";
                                }
                            }

                            PlasmaComponents3.Label {
                                Layout.fillWidth: true
                                elide: Text.ElideRight
                                opacity: 0.7
                                font: Kirigami.Theme.smallFont
                                visible: text.length > 0
                                text: {
                                    const b = fullRoot.backend;
                                    const parts = [];
                                    if (b.state === "connected" && b.activeIpv4) {
                                        parts.push(b.activeIpv4);
                                    }
                                    if (b.connectivityLabel) {
                                        parts.push(b.connectivityLabel);
                                    }
                                    if (b.hostname) {
                                        parts.push(b.hostname);
                                    }
                                    return parts.join(" · ");
                                }
                            }
                        }

                        PlasmaComponents3.BusyIndicator {
                            running: fullRoot.backend.busy
                            visible: running
                            implicitWidth: Kirigami.Units.iconSizes.smallMedium
                            implicitHeight: Kirigami.Units.iconSizes.smallMedium
                        }
                    }
                }

                // Action/daemon errors are shown inline; no modal message box.
                PlasmaComponents3.Frame {
                    Layout.fillWidth: true
                    visible: fullRoot.backend.lastError.length > 0
                    contentItem: RowLayout {
                        spacing: Kirigami.Units.smallSpacing

                        Kirigami.Icon {
                            source: "emblem-important-symbolic"
                            implicitWidth: Kirigami.Units.iconSizes.small
                            implicitHeight: Kirigami.Units.iconSizes.small
                        }

                        PlasmaComponents3.Label {
                            Layout.fillWidth: true
                            text: String(fullRoot.backend.lastError)
                            color: Kirigami.Theme.negativeTextColor
                            wrapMode: Text.WordWrap
                        }

                        PlasmaComponents3.ToolButton {
                            icon.name: "edit-clear-symbolic"
                            onClicked: fullRoot.backend.clearError()
                            PlasmaComponents3.ToolTip {
                                text: "忽略错误"
                            }
                        }
                    }
                }

                // Wi-Fi section.
                RowLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: Kirigami.Units.smallSpacing
                    spacing: Kirigami.Units.smallSpacing

                    PlasmaExtras.Heading {
                        level: 3
                        text: "Wi-Fi 网络"
                        Layout.fillWidth: true
                    }

                    PlasmaComponents3.Label {
                        visible: fullRoot.backend.daemonAvailable
                            && fullRoot.backend.wifiEnabled
                            && fullRoot.backend.hasWifiDevice
                        text: String(fullRoot.backend.wifiDevice)
                        opacity: 0.6
                        font: Kirigami.Theme.smallFont
                    }

                    PlasmaComponents3.BusyIndicator {
                        running: fullRoot.backend.busy
                        visible: running
                        implicitWidth: Kirigami.Units.iconSizes.smallMedium
                        implicitHeight: Kirigami.Units.iconSizes.smallMedium
                    }

                    PlasmaComponents3.Button {
                        text: "扫描"
                        icon.name: "view-refresh-symbolic"
                        enabled: fullRoot.backend.daemonAvailable
                            && fullRoot.backend.wifiEnabled
                            && fullRoot.backend.hasWifiDevice
                            && !fullRoot.backend.busy
                        onClicked: fullRoot.backend.scan()
                    }
                }

                PlasmaComponents3.Frame {
                    Layout.fillWidth: true
                    visible: !fullRoot.backend.wifiAvailable
                    contentItem: RowLayout {
                        spacing: Kirigami.Units.smallSpacing

                        Kirigami.Icon {
                            source: "network-wireless-disconnected-symbolic"
                            implicitWidth: Kirigami.Units.iconSizes.smallMedium
                            implicitHeight: Kirigami.Units.iconSizes.smallMedium
                        }

                        PlasmaComponents3.Label {
                            Layout.fillWidth: true
                            text: "Wi-Fi 不可用：未检测到无线硬件，或 wpa_supplicant 后端不可用。"
                            wrapMode: Text.WordWrap
                        }
                    }
                }

                PlasmaComponents3.Frame {
                    Layout.fillWidth: true
                    visible: fullRoot.backend.wifiAvailable && !fullRoot.backend.wifiEnabled
                    contentItem: RowLayout {
                        spacing: Kirigami.Units.smallSpacing

                        PlasmaComponents3.Label {
                            Layout.fillWidth: true
                            text: "Wi-Fi 已关闭。打开后可以扫描并连接附近网络。"
                            wrapMode: Text.WordWrap
                        }

                        PlasmaComponents3.Button {
                            text: "启用 Wi-Fi"
                            enabled: fullRoot.backend.daemonAvailable && !fullRoot.backend.busy
                            onClicked: fullRoot.backend.setWifiEnabled(true)
                        }
                    }
                }

                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    visible: fullRoot.backend.wifiAvailable
                        && fullRoot.backend.wifiEnabled
                        && !fullRoot.backend.hasWifiDevice
                    text: "没有可用的 Wi-Fi 接口。"
                    opacity: 0.7
                    wrapMode: Text.WordWrap
                }

                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    visible: fullRoot.backend.wifiAvailable
                        && fullRoot.backend.wifiEnabled
                        && fullRoot.backend.hasWifiDevice
                        && fullRoot.backend.wifiNetworks.length === 0
                        && !fullRoot.backend.busy
                    text: "未发现 Wi-Fi 网络。点击“扫描”查找附近网络。"
                    opacity: 0.7
                    wrapMode: Text.WordWrap
                }

                Repeater {
                    model: fullRoot.backend.wifiNetworks

                    delegate: PlasmaComponents3.ItemDelegate {
                        id: networkDelegate

                        required property var modelData

                        Layout.fillWidth: true
                        enabled: fullRoot.backend.daemonAvailable && !fullRoot.backend.busy
                        onClicked: fullRoot.activateNetwork(networkDelegate.modelData)
                        Accessible.name: networkDelegate.modelData.displaySsid
                            + "，" + networkDelegate.modelData.securityLabel
                            + "，信号 " + networkDelegate.modelData.signalLabel
                            + (networkDelegate.modelData.connected ? "，已连接" : "")

                        contentItem: RowLayout {
                            spacing: Kirigami.Units.smallSpacing

                            Kirigami.Icon {
                                source: networkDelegate.modelData.strengthIcon
                                implicitWidth: Kirigami.Units.iconSizes.smallMedium
                                implicitHeight: Kirigami.Units.iconSizes.smallMedium
                                Layout.alignment: Qt.AlignVCenter
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0

                                RowLayout {
                                    spacing: Kirigami.Units.smallSpacing

                                    PlasmaComponents3.Label {
                                        Layout.fillWidth: true
                                        text: networkDelegate.modelData.displaySsid
                                        font.bold: networkDelegate.modelData.connected
                                        elide: Text.ElideRight
                                    }

                                    Kirigami.Icon {
                                        visible: networkDelegate.modelData.requiresPassword
                                        source: "lock-symbolic"
                                        implicitWidth: Kirigami.Units.iconSizes.small
                                        implicitHeight: Kirigami.Units.iconSizes.small
                                    }
                                }

                                PlasmaComponents3.Label {
                                    Layout.fillWidth: true
                                    elide: Text.ElideRight
                                    opacity: 0.65
                                    font: Kirigami.Theme.smallFont
                                    text: {
                                        const n = networkDelegate.modelData;
                                        const parts = [n.securityLabel];
                                        if (n.bandLabel) {
                                            parts.push(n.bandLabel);
                                        }
                                        parts.push(n.signalLabel);
                                        if (n.connected) {
                                            parts.push("已连接");
                                        }
                                        return parts.join(" · ");
                                    }
                                }
                            }

                            PlasmaComponents3.Label {
                                text: networkDelegate.modelData.quality + "%"
                                opacity: 0.7
                                font: Kirigami.Theme.smallFont
                                Layout.alignment: Qt.AlignVCenter
                            }

                            PlasmaComponents3.ToolButton {
                                visible: networkDelegate.modelData.connected
                                icon.name: "network-disconnect-symbolic"
                                onClicked: fullRoot.backend.disconnectDevice(fullRoot.backend.wifiDevice)
                                PlasmaComponents3.ToolTip {
                                    text: "断开此 Wi-Fi 网络"
                                }
                            }
                        }
                    }
                }

                PlasmaComponents3.Button {
                    Layout.fillWidth: true
                    visible: fullRoot.backend.wifiAvailable
                        && fullRoot.backend.wifiEnabled
                        && fullRoot.backend.hasWifiDevice
                    text: "连接到隐藏网络…"
                    icon.name: "list-add-symbolic"
                    enabled: !fullRoot.backend.busy
                    onClicked: passwordDialog.openHidden()
                }

                // Device section.
                PlasmaExtras.Heading {
                    Layout.fillWidth: true
                    Layout.topMargin: Kirigami.Units.smallSpacing
                    level: 3
                    text: "网络设备"
                }

                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    visible: fullRoot.backend.devices.length === 0
                    text: "未检测到可管理的网络设备。"
                    opacity: 0.7
                }

                Repeater {
                    model: fullRoot.backend.devices

                    delegate: PlasmaComponents3.Frame {
                        id: deviceCard

                        required property var modelData

                        Layout.fillWidth: true

                        contentItem: RowLayout {
                            spacing: Kirigami.Units.smallSpacing

                            Kirigami.Icon {
                                source: deviceCard.modelData.iconName
                                implicitWidth: Kirigami.Units.iconSizes.smallMedium
                                implicitHeight: Kirigami.Units.iconSizes.smallMedium
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0

                                PlasmaComponents3.Label {
                                    Layout.fillWidth: true
                                    text: deviceCard.modelData.displayName
                                        + (deviceCard.modelData.active ? " · 活动" : "")
                                    font.bold: deviceCard.modelData.active
                                    elide: Text.ElideRight
                                }

                                PlasmaComponents3.Label {
                                    Layout.fillWidth: true
                                    elide: Text.ElideRight
                                    opacity: 0.65
                                    font: Kirigami.Theme.smallFont
                                    text: {
                                        const d = deviceCard.modelData;
                                        const parts = [d.ifname, d.kindLabel, d.stateLabel];
                                        if (d.primaryIpv4) {
                                            parts.push(d.primaryIpv4);
                                        }
                                        return parts.join(" · ");
                                    }
                                }

                                PlasmaComponents3.Label {
                                    Layout.fillWidth: true
                                    visible: text.length > 0
                                    text: deviceCard.modelData.lastError || ""
                                    color: Kirigami.Theme.negativeTextColor
                                    font: Kirigami.Theme.smallFont
                                    wrapMode: Text.WordWrap
                                }
                            }

                            PlasmaComponents3.Label {
                                text: "启用"
                                opacity: deviceCard.modelData.managed ? 0.8 : 0.4
                            }

                            PlasmaComponents3.Switch {
                                id: deviceEnableSwitch
                                Accessible.name: "启用 " + deviceCard.modelData.ifname
                                checked: deviceCard.modelData.enabled
                                enabled: fullRoot.backend.daemonAvailable
                                    && deviceCard.modelData.managed
                                    && !fullRoot.backend.busy
                                onToggled: {
                                    if (checked !== deviceCard.modelData.enabled) {
                                        fullRoot.backend.setDeviceEnabled(
                                            deviceCard.modelData.ifname, checked);
                                    }
                                }
                                PlasmaComponents3.ToolTip {
                                    text: deviceEnableSwitch.checked ? "禁用此设备" : "启用此设备"
                                }

                                Connections {
                                    target: fullRoot.backend
                                    function onLastErrorChanged() {
                                        if (fullRoot.backend.lastError.length > 0) {
                                            deviceEnableSwitch.checked = deviceCard.modelData.enabled;
                                        }
                                    }
                                }
                            }

                            PlasmaComponents3.Button {
                                visible: deviceCard.modelData.isEthernet
                                text: deviceCard.modelData.connected ? "断开" : "连接"
                                enabled: fullRoot.backend.daemonAvailable
                                    && deviceCard.modelData.managed
                                    && !fullRoot.backend.busy
                                    && (deviceCard.modelData.canConnect
                                        || deviceCard.modelData.canDisconnect)
                                onClicked: {
                                    const d = deviceCard.modelData;
                                    if (d.connected) {
                                        fullRoot.backend.disconnectDevice(d.ifname);
                                    } else {
                                        // The daemon starts DHCP when an Ethernet
                                        // device is enabled.
                                        fullRoot.backend.setDeviceEnabled(d.ifname, true);
                                    }
                                }
                            }

                            PlasmaComponents3.ToolButton {
                                visible: deviceCard.modelData.isWifi && deviceCard.modelData.connected
                                icon.name: "network-disconnect-symbolic"
                                onClicked: fullRoot.backend.disconnectDevice(deviceCard.modelData.ifname)
                                PlasmaComponents3.ToolTip {
                                    text: "断开 Wi-Fi"
                                }
                            }
                        }
                    }
                }

                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    Layout.topMargin: Kirigami.Units.smallSpacing
                    horizontalAlignment: Text.AlignHCenter
                    visible: text.length > 0
                    opacity: 0.45
                    font: Kirigami.Theme.smallFont
                    text: {
                        const b = fullRoot.backend;
                        if (!b.daemonAvailable) {
                            return "";
                        }
                        const parts = [];
                        if (b.daemonVersion) {
                            parts.push("netcore " + b.daemonVersion);
                        }
                        if (b.wpaBackend) {
                            parts.push("wpa: " + b.wpaBackend);
                        }
                        return parts.join(" · ");
                    }
                }
            }
        }

        PlasmaExtras.PlaceholderMessage {
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.gridUnit * 2
            visible: !fullRoot.backend.daemonAvailable
            iconName: "network-offline-symbolic"
            text: "netcore 守护进程未运行"
            explanation: "无法连接到系统总线上的 org.netcore.NetworkManager。\n"
                + "请启动 netcore 守护进程后重试。"
            helpfulAction: Kirigami.Action {
                text: "重试"
                icon.name: "view-refresh-symbolic"
                onTriggered: fullRoot.backend.refresh()
            }
        }
    }

    Connections {
        target: fullRoot.backend
        function onSnapshotChanged() {
            wifiSwitch.checked = fullRoot.backend.wifiEnabled;
        }
        function onLastErrorChanged() {
            if (fullRoot.backend.lastError.length > 0) {
                wifiSwitch.checked = fullRoot.backend.wifiEnabled;
            }
        }
    }

    PasswordDialog {
        id: passwordDialog
        backend: fullRoot.backend
        parent: fullRoot
    }
}
