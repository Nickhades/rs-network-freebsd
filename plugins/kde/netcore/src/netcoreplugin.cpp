/*
    SPDX-FileCopyrightText: 2026 netcore contributors
    SPDX-License-Identifier: BSD-2-Clause
*/

#include "netcoreplugin.h"
#include "netcorebackend.h"

#include <QQmlEngine>

void NetcorePlugin::registerTypes(const char *uri)
{
    Q_ASSERT(QLatin1String(uri) == QLatin1String("Netcore.Backend"));

    // A plain instantiable QML type: main.qml owns exactly one instance and
    // hands it to the compact and full representations. This keeps the
    // D-Bus watchers alive across panel popup open/close cycles and avoids a
    // process-wide singleton shared between unrelated applet instances.
    qmlRegisterType<NetcoreBackend>(uri, 1, 0, "NetcoreBackend");
}
