/*
    SPDX-FileCopyrightText: 2026 netcore contributors
    SPDX-License-Identifier: BSD-2-Clause
*/

import QtQuick
import QtQuick.Layouts
import org.kde.plasma.components as PlasmaComponents3
import org.kde.plasma.extras as PlasmaExtras
import org.kde.kirigami as Kirigami
import Netcore.Backend 1.0

/*!
    Native password prompt for a secured network (or for an explicitly hidden
    SSID). No JavaScript prompt is used, and nothing is persisted.
*/
PlasmaComponents3.Dialog {
    id: dialog

    required property NetcoreBackend backend

    property string ifname: ""
    property string ssid: ""
    property string security: ""
    property bool hiddenNetwork: false
    property bool requiresPassword: false

    title: hiddenNetwork ? "连接到隐藏网络" : "连接到 " + ssid
    modal: true
    width: Kirigami.Units.gridUnit * 21
    // PlasmaComponents3.Dialog's implicitHeight does not include the footer;
    // account for it explicitly so buttons never overlap the content.
    height: implicitHeight + (footer ? footer.implicitHeight : 0)
    x: parent ? Math.round((parent.width - width) / 2) : 0
    y: parent ? Math.round((parent.height - height) / 2) : 0

    function openFor(network) {
        if (!network) {
            return;
        }
        ifname = backend.wifiDevice;
        ssid = network.ssid;
        security = network.security;
        requiresPassword = network.requiresPassword;
        hiddenNetwork = false;
        ssidField.text = network.ssid;
        passwordField.text = "";
        open();
    }

    function openHidden() {
        ifname = backend.wifiDevice;
        ssid = "";
        security = "unknown";
        requiresPassword = false;
        hiddenNetwork = true;
        ssidField.text = "";
        passwordField.text = "";
        open();
    }

    function canSubmit() {
        if (!backend.daemonAvailable || backend.busy) {
            return false;
        }
        if (hiddenNetwork && ssidField.text.trim().length === 0) {
            return false;
        }
        if (requiresPassword && passwordField.text.length === 0) {
            return false;
        }
        return true;
    }

    function submit() {
        if (!canSubmit()) {
            return;
        }
        const targetSsid = hiddenNetwork ? ssidField.text.trim() : ssid;
        backend.connectWifi(ifname, targetSsid, passwordField.text, hiddenNetwork);
        passwordField.text = "";
        close();
    }

    onOpened: {
        if (hiddenNetwork) {
            ssidField.forceActiveFocus();
        } else {
            passwordField.forceActiveFocus();
        }
    }
    onClosed: passwordField.text = ""

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.smallSpacing
        implicitWidth: Kirigami.Units.gridUnit * 18

        PlasmaComponents3.Label {
            Layout.fillWidth: true
            text: dialog.hiddenNetwork
                ? "请输入隐藏网络的名称（SSID）。密码可以留空，由守护进程判断网络是否需要密码。"
                : "请输入“" + dialog.ssid + "”的密码。"
            wrapMode: Text.WordWrap
        }

        PlasmaComponents3.TextField {
            id: ssidField
            Layout.fillWidth: true
            visible: dialog.hiddenNetwork
            placeholderText: "网络名称（SSID）"
            inputMethodHints: Qt.ImhNoAutoUppercase | Qt.ImhNoPredictiveText
            onAccepted: passwordField.forceActiveFocus()
        }

        PlasmaExtras.PasswordField {
            id: passwordField
            Layout.fillWidth: true
            placeholderText: dialog.requiresPassword ? "Wi-Fi 密码" : "密码（开放网络可留空）"
            onAccepted: dialog.submit()
        }

        PlasmaComponents3.Label {
            Layout.fillWidth: true
            visible: dialog.security === "enterprise"
            text: "企业级网络通常需要 802.1X 身份配置，当前 ConnectWifi 接口只接受密码。"
            wrapMode: Text.WordWrap
            color: Kirigami.Theme.neutralTextColor
            font: Kirigami.Theme.smallFont
        }

        PlasmaComponents3.Label {
            Layout.fillWidth: true
            text: "密码只会通过系统总线发送给 netcore 守护进程，不会保存在本地。"
            wrapMode: Text.WordWrap
            opacity: 0.6
            font: Kirigami.Theme.smallFont
        }
    }

    footer: PlasmaComponents3.DialogButtonBox {
        PlasmaComponents3.Button {
            text: "取消"
            onClicked: dialog.close()
        }
        PlasmaComponents3.Button {
            text: "连接"
            highlighted: true
            enabled: dialog.canSubmit()
            onClicked: dialog.submit()
        }
    }
}
