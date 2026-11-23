/*
    SPDX-FileCopyrightText: 2026 netcore contributors
    SPDX-License-Identifier: BSD-2-Clause
*/

#pragma once

#include <QQmlExtensionPlugin>

/*!
 * QML extension plugin exporting the Netcore.Backend 1.0 module.
 */
class NetcorePlugin final : public QQmlExtensionPlugin
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID "org.qt-project.Qt.QQmlExtensionInterface/1.0")

public:
    void registerTypes(const char *uri) override;
};
