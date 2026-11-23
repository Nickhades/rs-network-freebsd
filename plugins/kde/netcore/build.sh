#!/bin/sh
# SPDX-FileCopyrightText: 2026 netcore contributors
# SPDX-License-Identifier: BSD-2-Clause
#
# Build/install netcore's Plasma 6 applet without requiring CMake.
# CMakeLists.txt is the primary build system; this script is a dependency-light
# fallback for FreeBSD hosts where cmake is not installed.
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BUILD_DIR=${BUILD_DIR:-"$SCRIPT_DIR/build"}
PREFIX=${PREFIX:-/usr/local}
DESTDIR=${DESTDIR:-}
QML_DIR=${QML_DIR:-}
CXX=${CXX:-c++}
PKG_CONFIG=${PKG_CONFIG:-pkg-config}

UI_DIR="$SCRIPT_DIR/contents/ui"
SRC_DIR="$SCRIPT_DIR/src"
QML_STAGE="$BUILD_DIR/Netcore/Backend"

usage() {
    cat <<'EOF'
用法: ./build.sh [选项]

选项:
  --build     构建 Netcore.Backend QML 插件（默认）
  --check     构建后运行 qmllint / qmlcachegen / qmlformat / JSON 校验
  --install   构建并安装 QML 模块与 Plasma 小程序
  --clean     删除构建目录
  --help      显示此帮助

环境变量:
  PREFIX      安装前缀，默认 /usr/local（Plasma 小程序安装到 share/plasma/plasmoids）
  DESTDIR     打包暂存目录，默认空
  QML_DIR     QML 模块安装目录，默认 Qt 的 QT_INSTALL_QML（通常 /usr/local/lib/qt6/qml）
  BUILD_DIR   构建目录，默认 ./build
  CXX         C++ 编译器，默认 c++
  QMAKE       qmake6 路径，自动探测
  QMLLINT / QMLCACHEGEN / QMLFORMAT / MOC  可显式指定工具路径
EOF
}

die() {
    printf '错误: %s\n' "$*" >&2
    exit 1
}

detect_qt() {
    if [ -z "${QMAKE:-}" ]; then
        if command -v qmake6 >/dev/null 2>&1; then
            QMAKE=$(command -v qmake6)
        elif [ -x /usr/local/lib/qt6/bin/qmake6 ]; then
            QMAKE=/usr/local/lib/qt6/bin/qmake6
        elif command -v qmake >/dev/null 2>&1; then
            QMAKE=$(command -v qmake)
        else
            die "未找到 qmake6，请安装 Qt 6 开发包"
        fi
    fi

    QT_INSTALL_PREFIX=$("$QMAKE" -query QT_INSTALL_PREFIX)
    QT_INSTALL_QML=$("$QMAKE" -query QT_INSTALL_QML)
    QT_INSTALL_LIBEXECS=$("$QMAKE" -query QT_INSTALL_LIBEXECS)
    QT_INSTALL_BINS=$("$QMAKE" -query QT_INSTALL_BINS)

    case "$QT_INSTALL_QML" in
        /*) ;;
        *) QT_INSTALL_QML="$QT_INSTALL_PREFIX/$QT_INSTALL_QML" ;;
    esac

    if [ -z "${MOC:-}" ]; then
        if [ -x "$QT_INSTALL_LIBEXECS/moc" ]; then
            MOC="$QT_INSTALL_LIBEXECS/moc"
        elif command -v moc >/dev/null 2>&1; then
            MOC=$(command -v moc)
        else
            die "未找到 moc"
        fi
    fi

    QMLLINT=${QMLLINT:-"$QT_INSTALL_BINS/qmllint"}
    QMLFORMAT=${QMLFORMAT:-"$QT_INSTALL_BINS/qmlformat"}
    QMLCACHEGEN=${QMLCACHEGEN:-"$QT_INSTALL_LIBEXECS/qmlcachegen"}

    [ -n "${QML_DIR:-}" ] || QML_DIR="$QT_INSTALL_QML"

    command -v "$CXX" >/dev/null 2>&1 || die "未找到 C++ 编译器: $CXX"
    "$PKG_CONFIG" --exists Qt6Core Qt6DBus Qt6Qml \
        || die "pkg-config 找不到 Qt6Core/Qt6DBus/Qt6Qml 开发文件"
}

build_plugin() {
    detect_qt
    echo "== 构建 Netcore.Backend QML 插件 =="
    echo "Qt: $QT_INSTALL_PREFIX"
    echo "构建目录: $BUILD_DIR"

    mkdir -p "$BUILD_DIR" "$QML_STAGE"

    "$MOC" "$SRC_DIR/netcorebackend.h" -o "$BUILD_DIR/moc_netcorebackend.cpp"
    "$MOC" "$SRC_DIR/netcoreplugin.h" -o "$BUILD_DIR/moc_netcoreplugin.cpp"

    QT_CFLAGS=$("$PKG_CONFIG" --cflags Qt6Core Qt6DBus Qt6Qml)
    QT_LIBS=$("$PKG_CONFIG" --libs Qt6Core Qt6DBus Qt6Qml)

    # shellcheck disable=SC2086
    "$CXX" -std=c++17 -fPIC -shared -I"$SRC_DIR" \
        -o "$QML_STAGE/libnetcorebackendplugin.so" \
        "$SRC_DIR/netcorebackend.cpp" \
        "$SRC_DIR/netcoreplugin.cpp" \
        "$BUILD_DIR/moc_netcorebackend.cpp" \
        "$BUILD_DIR/moc_netcoreplugin.cpp" \
        $QT_CFLAGS $QT_LIBS

    cp "$SCRIPT_DIR/qml/qmldir" "$QML_STAGE/qmldir"
    cp "$SCRIPT_DIR/qml/netcorebackend.qmltypes" "$QML_STAGE/netcorebackend.qmltypes"

    echo "构建完成: $QML_STAGE/libnetcorebackendplugin.so"
}

run_checks() {
    detect_qt
    [ -x "$QMLLINT" ] || die "未找到 qmllint: $QMLLINT"
    [ -x "$QMLCACHEGEN" ] || die "未找到 qmlcachegen: $QMLCACHEGEN"

    printf '== JSON 校验 ==\n'
    if command -v python3 >/dev/null 2>&1; then
        python3 -m json.tool "$SCRIPT_DIR/metadata.json" >/dev/null
        echo "metadata.json: OK"
    else
        echo "跳过: 未找到 python3"
    fi

    printf '== qmllint ==\n'
    "$QMLLINT" -I "$BUILD_DIR" -I "$QT_INSTALL_QML" "$UI_DIR"/*.qml || exit 1

    printf '== qmlcachegen ==\n'
    QMLC_TMP=$(mktemp -d "${TMPDIR:-/tmp}/netcore-qmlc.XXXXXX")
    trap 'rm -rf "$QMLC_TMP"' EXIT HUP INT TERM
    for qml in "$UI_DIR"/*.qml; do
        "$QMLCACHEGEN" -I "$BUILD_DIR" -I "$QT_INSTALL_QML" \
            "$qml" -o "$QMLC_TMP/$(basename "$qml").qmlc"
        echo "qmlcachegen: $(basename "$qml") OK"
    done
    rm -rf "$QMLC_TMP"
    trap - EXIT HUP INT TERM

    if [ -x "$QMLFORMAT" ]; then
        printf '== qmlformat 语法检查 ==\n'
        for qml in "$UI_DIR"/*.qml; do
            "$QMLFORMAT" "$qml" >/dev/null
            echo "qmlformat: $(basename "$qml") OK"
        done
    fi

    echo "所有检查完成。"
}

install_all() {
    build_plugin
    [ "$(id -u)" -eq 0 ] || [ -n "$DESTDIR" ] \
        || echo "提示: 没有 DESTDIR 且非 root，安装到 $PREFIX 可能需要 sudo。"

    DEST_QML="$DESTDIR$QML_DIR/Netcore/Backend"
    DEST_PLASMOID="$DESTDIR$PREFIX/share/plasma/plasmoids/org.netcore.networkmanager"

    echo "== 安装 QML 模块 =="
    install -d "$DEST_QML"
    install -m 755 "$QML_STAGE/libnetcorebackendplugin.so" "$DEST_QML/"
    install -m 644 "$SCRIPT_DIR/qml/qmldir" "$DEST_QML/"
    install -m 644 "$SCRIPT_DIR/qml/netcorebackend.qmltypes" "$DEST_QML/"

    echo "== 安装 Plasma 小程序 =="
    install -d "$DEST_PLASMOID/contents/ui"
    install -m 644 "$SCRIPT_DIR/metadata.json" "$DEST_PLASMOID/"
    install -m 644 "$UI_DIR"/*.qml "$DEST_PLASMOID/contents/ui/"

    echo "QML 模块: $DEST_QML"
    echo "Plasma 小程序: $DEST_PLASMOID"
    echo "重新启动 plasmashell 或在“添加小组件”中重新加载后才能看到变更。"
}

ACTION=build
for arg in "$@"; do
    case "$arg" in
        --build) ACTION=build ;;
        --check) ACTION=check ;;
        --install) ACTION=install ;;
        --clean) ACTION=clean ;;
        --help|-h) usage; exit 0 ;;
        *) usage >&2; die "未知选项: $arg" ;;
    esac
done

case "$ACTION" in
    build) build_plugin ;;
    check) build_plugin; run_checks ;;
    install) install_all ;;
    clean)
        [ -n "$BUILD_DIR" ] && [ "$BUILD_DIR" != "/" ] || die "拒绝删除 $BUILD_DIR"
        rm -rf "$BUILD_DIR"
        echo "已删除 $BUILD_DIR"
        ;;
esac
