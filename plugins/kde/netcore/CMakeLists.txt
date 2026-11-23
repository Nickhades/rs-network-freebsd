# SPDX-FileCopyrightText: 2026 netcore contributors
# SPDX-License-Identifier: BSD-2-Clause

cmake_minimum_required(VERSION 3.16)

project(netcore-kde-plugin VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

include(GNUInstallDirs)

find_package(Qt6 6.5 REQUIRED COMPONENTS Core DBus Qml)

# ---------------------------------------------------------------------------
# Netcore.Backend QML module (C++ QQmlExtensionPlugin)
# ---------------------------------------------------------------------------
add_library(netcorebackendplugin SHARED
    src/netcorebackend.cpp
    src/netcorebackend.h
    src/netcoreplugin.cpp
    src/netcoreplugin.h
)

target_include_directories(netcorebackendplugin PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/src")
target_link_libraries(netcorebackendplugin PRIVATE Qt6::Core Qt6::DBus Qt6::Qml)

set_target_properties(netcorebackendplugin PROPERTIES
    OUTPUT_NAME netcorebackendplugin
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/qml/Netcore/Backend"
)

# Put a usable, qmllint-able module into the build tree as well.
add_custom_command(TARGET netcorebackendplugin POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${CMAKE_CURRENT_SOURCE_DIR}/qml/qmldir"
        "$<TARGET_FILE_DIR:netcorebackendplugin>/qmldir"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${CMAKE_CURRENT_SOURCE_DIR}/qml/netcorebackend.qmltypes"
        "$<TARGET_FILE_DIR:netcorebackendplugin>/netcorebackend.qmltypes"
)

# ---------------------------------------------------------------------------
# Install paths
#
# The QML module must be visible to plasmashell's QML import path. On FreeBSD
# that is /usr/local/lib/qt6/qml. QT6_INSTALL_QML is relative on some Qt
# builds, so resolve it against the Qt prefix instead of CMAKE_INSTALL_PREFIX.
# A packager can override NETCORE_QML_INSTALL_DIR explicitly.
# ---------------------------------------------------------------------------
if(DEFINED QT6_INSTALL_QML AND IS_ABSOLUTE "${QT6_INSTALL_QML}")
    set(_netcore_qml_default "${QT6_INSTALL_QML}")
elseif(DEFINED QT6_INSTALL_PREFIX AND DEFINED QT6_INSTALL_QML)
    set(_netcore_qml_default "${QT6_INSTALL_PREFIX}/${QT6_INSTALL_QML}")
else()
    set(_netcore_qml_default "${CMAKE_INSTALL_PREFIX}/lib/qt6/qml")
endif()

set(NETCORE_QML_INSTALL_DIR "${_netcore_qml_default}/Netcore/Backend"
    CACHE PATH "Directory where the Netcore.Backend QML module is installed")

install(TARGETS netcorebackendplugin
    LIBRARY DESTINATION "${NETCORE_QML_INSTALL_DIR}"
)

install(FILES
    qml/qmldir
    qml/netcorebackend.qmltypes
    DESTINATION "${NETCORE_QML_INSTALL_DIR}"
)

# ---------------------------------------------------------------------------
# Plasma applet package
# ---------------------------------------------------------------------------
set(NETCORE_PLASMOID_INSTALL_DIR
    "${CMAKE_INSTALL_DATADIR}/plasma/plasmoids/org.netcore.networkmanager")

install(FILES metadata.json DESTINATION "${NETCORE_PLASMOID_INSTALL_DIR}")
install(DIRECTORY contents DESTINATION "${NETCORE_PLASMOID_INSTALL_DIR}")

message(STATUS "Netcore.Backend QML module: ${NETCORE_QML_INSTALL_DIR}")
message(STATUS "Plasma applet: ${CMAKE_INSTALL_PREFIX}/${NETCORE_PLASMOID_INSTALL_DIR}")
