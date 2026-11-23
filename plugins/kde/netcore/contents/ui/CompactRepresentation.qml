/*
    SPDX-FileCopyrightText: 2026 netcore contributors
    SPDX-License-Identifier: BSD-2-Clause
*/

import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Netcore.Backend 1.0

/*!
    Panel representation: only a symbolic status icon. The full representation
    is opened by the applet itself through the activated() signal.
*/
Item {
    id: root

    required property NetcoreBackend backend
    signal activated

    implicitWidth: Kirigami.Units.iconSizes.smallMedium
    implicitHeight: Kirigami.Units.iconSizes.smallMedium
    Layout.fillWidth: true
    Layout.fillHeight: true

    Accessible.name: backend.daemonAvailable
        ? backend.toolTipTitle + "，" + backend.toolTipSubText
        : "netcore 网络管理器，守护进程未运行"
    Accessible.role: Accessible.Button
    Accessible.onPressAction: root.activated()

    activeFocusOnTab: true
    Keys.onSpacePressed: root.activated()
    Keys.onReturnPressed: root.activated()

    Kirigami.Icon {
        anchors.centerIn: parent
        width: Math.min(parent.width, parent.height)
        height: width
        source: root.backend.compactIcon
        active: hoverArea.containsMouse || root.activeFocus
    }

    MouseArea {
        id: hoverArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.activated()
    }
}
