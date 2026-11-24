/*
    SPDX-FileCopyrightText: 2026 netcore contributors
    SPDX-License-Identifier: BSD-2-Clause
*/

pragma ComponentBehavior: Bound

import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import Netcore.Backend 1.0

/*!
    netcore network manager applet.

    The backend object lives here, at the applet root, so its system-bus
    connections and periodic fallback survive the panel popup opening and
    closing. Both representations only borrow it.
*/
PlasmoidItem {
    id: root

    NetcoreBackend {
        id: netcoreBackend
    }

    Plasmoid.title: "网络"
    Plasmoid.backgroundHints: PlasmaCore.Types.DefaultBackground
        | PlasmaCore.Types.ConfigurableBackground

    toolTipMainText: String(netcoreBackend.toolTipTitle)
    toolTipSubText: String(netcoreBackend.toolTipSubText)
    toolTipTextFormat: Text.PlainText

    compactRepresentation: CompactRepresentation {
        backend: netcoreBackend
        onActivated: root.expanded = !root.expanded
    }

    fullRepresentation: FullRepresentation {
        backend: netcoreBackend
    }

    onExpandedChanged: {
        if (root.expanded) {
            netcoreBackend.refresh();
        }
    }

    Plasmoid.contextualActions: [
        PlasmaCore.Action {
            text: "刷新网络状态"
            icon.name: "view-refresh"
            onTriggered: netcoreBackend.refresh()
        },
        PlasmaCore.Action {
            text: "扫描 Wi-Fi"
            icon.name: "network-wireless"
            onTriggered: netcoreBackend.scan()
        },
        PlasmaCore.Action {
            text: "打开 netcore-gui"
            icon.name: "preferences-system-network"
            onTriggered: netcoreBackend.openGui()
        }
    ]
}