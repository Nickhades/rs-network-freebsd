#!/bin/sh
set -eu

# netcore FreeBSD installer.
#
# Modes:
#   auto   (default) detect a complete build environment; build if possible,
#          otherwise install existing prebuilt artifacts.
#   source force a source build. Never installs stale prebuilt artifacts.
#   binary install already-built artifacts only. No compiler is invoked.
#
# PREFIX and DESTDIR can also be supplied through the environment.
# DESTDIR allows installing into a staging root without root privileges.

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
cd "$ROOT"

MODE=auto
MODE_SET=no
CHECK_ONLY=no
PREFIX="${PREFIX:-/usr/local}"
DESTDIR="${DESTDIR:-}"
CARGO="${CARGO:-cargo}"
CC="${CC:-cc}"
CXX="${CXX:-c++}"

TARGET_DIR="${CARGO_TARGET_DIR:-target}"
case "$TARGET_DIR" in
    /*) ;;
    *) TARGET_DIR="$ROOT/$TARGET_DIR" ;;
esac
RELEASE_DIR="$TARGET_DIR/release"
KDE_DIR="$ROOT/plugins/kde/netcore"
KDE_SOURCE_BUILD_DIR="$KDE_DIR/build"
KDE_PREBUILT_PLUGIN="$KDE_SOURCE_BUILD_DIR/Netcore/Backend/libnetcorebackendplugin.so"
KDE_BUILD_DIR="$TARGET_DIR/kde-qml"
KDE_BUILT_PLUGIN="$KDE_BUILD_DIR/Netcore/Backend/libnetcorebackendplugin.so"
KDE_PLUGIN=""

CORE_BUILDABLE=no
KDE_BUILDABLE=no
MISSING_CORE=""
MISSING_KDE=""
PREBUILT_MISSING=""
INSTALL_KDE=no

usage() {
    cat <<'EOF'
netcore FreeBSD 一键安装

用法：
  sudo ./scripts/install-freebsd.sh [选项]            # 自动检测/编译安装
  sudo ./scripts/install-freebsd-prebuilt.sh [选项]   # 直接安装已编译产物

模式：
  --auto              自动检测编译环境：可编译则编译，否则安装已有产物（默认）
  --source, --build   强制从源码编译安装；编译环境不完整即失败
  --binary, --prebuilt 只安装已经编译好的产物；绝不调用编译器

其他：
  --check             只检测环境与预编译产物，不做编译或安装
  --prefix PATH       安装前缀，默认 /usr/local
  --destdir PATH      暂存根目录，适合打包或非 root 安装
  -h, --help          显示本帮助

环境变量：
  PREFIX、DESTDIR、CARGO、CARGO_TARGET_DIR、CC、CXX

自动模式检测项：
  cargo、rustc、C 编译器、pkg-config、GTK4、GTK3、libxfce4panel-2.0。
  Qt6/KDE 构建环境单独检测；缺失时只跳过 KDE 面板插件。
  编译环境不完整但 target/release 产物齐全时，自动改用预编译安装。
  两者都不满足时给出缺失清单并退出。

预编译产物位置：
  target/release/netcore
  target/release/netcore-gui
  target/release/netcore-daemon
  target/release/libnetcore_xfce_plugin.so
  plugins/kde/netcore/build/Netcore/Backend/libnetcorebackendplugin.so（可选）

示例：
  sudo ./scripts/install-freebsd.sh
  sudo ./scripts/install-freebsd.sh --source
  sudo ./scripts/install-freebsd-prebuilt.sh
  sudo ./scripts/install-freebsd.sh --check
  DESTDIR=/tmp/netcore-stage ./scripts/install-freebsd.sh --binary
EOF
}

die() {
    printf '错误: %s\n' "$*" >&2
    exit 1
}

info() {
    printf '==> %s\n' "$*"
}

warn() {
    printf '警告: %s\n' "$*" >&2
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --auto|--source|--build|--binary|--prebuilt)
            case "$1" in
                --auto) new_mode=auto ;;
                --source|--build) new_mode=source ;;
                --binary|--prebuilt) new_mode=binary ;;
            esac
            if [ "$MODE_SET" = yes ] && [ "$MODE" != "$new_mode" ]; then
                die "安装模式参数冲突：$MODE 与 $new_mode"
            fi
            MODE="$new_mode"
            MODE_SET=yes
            ;;
        --check)
            CHECK_ONLY=yes
            ;;
        --prefix)
            [ "$#" -ge 2 ] || die "--prefix 需要一个目录参数"
            PREFIX="$2"
            shift
            ;;
        --destdir)
            [ "$#" -ge 2 ] || die "--destdir 需要一个目录参数"
            DESTDIR="$2"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "未知参数: $1（使用 --help 查看用法）"
            ;;
    esac
    shift
done

export PREFIX DESTDIR

resolve_cargo() {
    # Under sudo a user-installed cargo can be outside root's secure PATH.
    if command -v "$CARGO" >/dev/null 2>&1; then
        return 0
    fi
    for candidate in /usr/local/bin/cargo /usr/bin/cargo "${HOME:-}/.cargo/bin/cargo"; do
        [ -n "$candidate" ] || continue
        if [ -x "$candidate" ]; then
            CARGO="$candidate"
            return 0
        fi
    done
    return 1
}

detect_core_env() {
    MISSING_CORE=""
    if ! resolve_cargo; then
        MISSING_CORE="$MISSING_CORE cargo"
    fi
    if ! command -v rustc >/dev/null 2>&1; then
        MISSING_CORE="$MISSING_CORE rustc"
    fi
    if ! command -v "$CC" >/dev/null 2>&1; then
        MISSING_CORE="$MISSING_CORE $CC"
    fi
    if command -v pkg-config >/dev/null 2>&1; then
        for package in gtk4 gtk+-3.0 libxfce4panel-2.0; do
            if ! pkg-config --exists "$package" >/dev/null 2>&1; then
                MISSING_CORE="$MISSING_CORE pkg-config:$package"
            fi
        done
    else
        MISSING_CORE="$MISSING_CORE pkg-config"
    fi
    if [ -z "$MISSING_CORE" ]; then
        CORE_BUILDABLE=yes
    else
        CORE_BUILDABLE=no
    fi
    return 0
}

find_qmake() {
    if command -v qmake6 >/dev/null 2>&1; then
        return 0
    fi
    if [ -x /usr/local/lib/qt6/bin/qmake6 ]; then
        return 0
    fi
    if command -v qmake >/dev/null 2>&1; then
        return 0
    fi
    return 1
}

find_moc() {
    if command -v moc >/dev/null 2>&1; then
        return 0
    fi
    for qmake in qmake6 /usr/local/lib/qt6/bin/qmake6 qmake; do
        command -v "$qmake" >/dev/null 2>&1 || continue
        libexec=$("$qmake" -query QT_INSTALL_LIBEXECS 2>/dev/null || true)
        if [ -n "$libexec" ] && [ -x "$libexec/moc" ]; then
            return 0
        fi
    done
    for candidate in /usr/local/libexec/qt6/moc /usr/local/lib/qt6/libexec/moc; do
        [ -x "$candidate" ] && return 0
    done
    return 1
}

detect_kde_env() {
    MISSING_KDE=""
    if ! find_qmake; then
        MISSING_KDE="$MISSING_KDE qmake6"
    fi
    if ! find_moc; then
        MISSING_KDE="$MISSING_KDE moc"
    fi
    if ! command -v "$CXX" >/dev/null 2>&1; then
        MISSING_KDE="$MISSING_KDE $CXX"
    fi
    if command -v pkg-config >/dev/null 2>&1; then
        if ! pkg-config --exists Qt6Core Qt6DBus Qt6Qml >/dev/null 2>&1; then
            MISSING_KDE="$MISSING_KDE Qt6Core/Qt6DBus/Qt6Qml"
        fi
    else
        MISSING_KDE="$MISSING_KDE pkg-config"
    fi
    if [ -z "$MISSING_KDE" ]; then
        KDE_BUILDABLE=yes
    else
        KDE_BUILDABLE=no
    fi
    return 0
}

check_prebuilt_core() {
    PREBUILT_MISSING=""
    for artifact in         "$RELEASE_DIR/netcore"         "$RELEASE_DIR/netcore-gui"         "$RELEASE_DIR/netcore-daemon"         "$RELEASE_DIR/libnetcore_xfce_plugin.so"
    do
        if [ ! -s "$artifact" ]; then
            PREBUILT_MISSING="$PREBUILT_MISSING $artifact"
        fi
    done
    [ -z "$PREBUILT_MISSING" ]
}

kde_prebuilt_available() {
    [ -s "$KDE_PREBUILT_PLUGIN" ] || [ -s "$KDE_BUILT_PLUGIN" ]
}

select_prebuilt_kde() {
    if [ -s "$KDE_BUILT_PLUGIN" ]; then
        KDE_PLUGIN="$KDE_BUILT_PLUGIN"
    elif [ -s "$KDE_PREBUILT_PLUGIN" ]; then
        KDE_PLUGIN="$KDE_PREBUILT_PLUGIN"
    else
        KDE_PLUGIN=""
    fi
}

build_core() {
    info "构建 Rust workspace：$CARGO build --release --workspace --locked"
    "$CARGO" build --release --workspace --locked
}

build_kde() {
    if [ "$KDE_BUILDABLE" != yes ]; then
        warn "未检测到完整的 Qt6/KDE 构建环境。"
        if [ -n "$MISSING_KDE" ]; then
            warn "缺少:$MISSING_KDE"
        fi
        if [ "$MODE" = auto ] && kde_prebuilt_available; then
            select_prebuilt_kde
            warn "使用已有的 KDE 预编译插件：$KDE_PLUGIN"
            INSTALL_KDE=yes
        else
            warn "跳过 KDE Plasma 插件。"
            INSTALL_KDE=no
        fi
        return 0
    fi
    info "构建 KDE Plasma QML 插件"
    if (cd "$KDE_DIR" && BUILD_DIR="$KDE_BUILD_DIR" ./build.sh --build); then
        if [ -s "$KDE_BUILT_PLUGIN" ]; then
            KDE_PLUGIN="$KDE_BUILT_PLUGIN"
            INSTALL_KDE=yes
            return 0
        fi
        warn "KDE 插件构建完成但未找到产物：$KDE_BUILT_PLUGIN"
    else
        warn "KDE 插件构建失败。"
    fi
    if [ "$MODE" = auto ] && kde_prebuilt_available; then
        select_prebuilt_kde
        warn "改用已有的 KDE 预编译插件：$KDE_PLUGIN"
        INSTALL_KDE=yes
    else
        warn "跳过 KDE Plasma 插件。"
        INSTALL_KDE=no
    fi
}

check_artifact_set() {
    if ! check_prebuilt_core; then
        die "缺少预编译产物：$PREBUILT_MISSING
请先运行：sudo ./scripts/install-freebsd.sh --source
或运行 ./scripts/install-freebsd.sh --auto 让脚本自动编译。"
    fi
}

detect_core_env
detect_kde_env
check_prebuilt_core || true

if [ "$CHECK_ONLY" = yes ]; then
    printf '%s\n' "netcore 安装环境检测"
    printf '  安装前缀: %s\n' "$PREFIX"
    printf '  暂存根目录: %s\n' "${DESTDIR:-（未设置，直接安装）}"
    printf '  Cargo 目标目录: %s\n' "$TARGET_DIR"
    if [ "$CORE_BUILDABLE" = yes ]; then
        printf '%s\n' "  Rust/GTK 编译环境: 完整"
    else
        printf '%s\n' "  Rust/GTK 编译环境: 不完整（$MISSING_CORE）"
    fi
    if [ "$KDE_BUILDABLE" = yes ]; then
        printf '%s\n' "  KDE 构建环境: 完整"
    else
        printf '%s\n' "  KDE 构建环境: 不完整（$MISSING_KDE）"
    fi
    if [ -z "$PREBUILT_MISSING" ]; then
        printf '%s\n' "  预编译核心产物: 完整"
        if kde_prebuilt_available; then
            printf '%s\n' "  预编译 KDE 插件: 存在"
        else
            printf '%s\n' "  预编译 KDE 插件: 缺失（安装时跳过 KDE）"
        fi
    else
        printf '%s\n' "  预编译核心产物: 缺失（$PREBUILT_MISSING）"
    fi
    case "$MODE" in
        auto)
            if [ "$CORE_BUILDABLE" = yes ]; then
                printf '%s\n' "  自动模式将: 编译并安装"
            elif [ -z "$PREBUILT_MISSING" ]; then
                printf '%s\n' "  自动模式将: 安装预编译产物"
            else
                printf '%s\n' "  自动模式将: 失败（编译环境和预编译产物均不可用）"
            fi
            ;;
        source)
            if [ "$CORE_BUILDABLE" = yes ]; then
                printf '%s\n' "  源码模式将: 编译并安装"
            else
                printf '%s\n' "  源码模式将: 失败（编译环境不完整）"
            fi
            ;;
        binary)
            if [ -z "$PREBUILT_MISSING" ]; then
                printf '%s\n' "  预编译模式将: 直接安装"
            else
                printf '%s\n' "  预编译模式将: 失败（产物缺失）"
            fi
            ;;
    esac
    exit 0
fi

if [ -z "$DESTDIR" ] && [ "$(id -u)" -ne 0 ]; then
    die "安装到 $PREFIX 需要 root 权限。请使用 sudo，或设置 DESTDIR 进行暂存安装。"
fi

if [ "$PREFIX" != "/usr/local" ]; then
    warn "服务文件和 GUI 桌面文件按 /usr/local 编写；安装到 $PREFIX 后需自行调整 Exec 路径。"
fi

case "$MODE" in
    source)
        detect_core_env
        if [ "$CORE_BUILDABLE" != yes ]; then
            die "编译环境不完整，缺少:$MISSING_CORE"
        fi
        detect_kde_env
        build_core
        build_kde
        ;;
    binary)
        : # No toolchain detection or build.
        if [ -z "$PREBUILT_MISSING" ]; then
            info "使用预编译产物：$RELEASE_DIR"
        else
            check_artifact_set
        fi
        if kde_prebuilt_available; then
            select_prebuilt_kde
            INSTALL_KDE=yes
        else
            INSTALL_KDE=no
            warn "未找到预编译 KDE 插件，跳过 KDE Plasma 集成。"
        fi
        ;;
    auto)
        if [ "$CORE_BUILDABLE" = yes ]; then
            info "检测到完整 Rust/GTK 编译环境，选择源码构建。"
            build_core
            build_kde
        elif [ -z "$PREBUILT_MISSING" ]; then
            warn "编译环境不完整（缺少:$MISSING_CORE），回退到预编译产物。"
            if kde_prebuilt_available; then
                select_prebuilt_kde
                INSTALL_KDE=yes
            else
                warn "未找到预编译 KDE 插件，跳过 KDE Plasma 集成。"
            fi
        else
            die "编译环境和预编译产物均不可用。
缺少编译依赖:$MISSING_CORE
缺少预编译产物:$PREBUILT_MISSING"
        fi
        ;;
    *)
        die "内部错误：未知模式 $MODE"
        ;;
esac

check_artifact_set

install_dir() {
    install -d "$DESTDIR$1"
}

install_file() {
    install -m "${3:-755}" "$1" "$DESTDIR$2"
}

info "安装到 ${DESTDIR:-}$PREFIX"
install_dir "$PREFIX/bin"
install_dir "$PREFIX/sbin"
install_dir "$PREFIX/etc/rc.d"
install_dir "$PREFIX/etc/dbus-1/system.d"
install_dir "$PREFIX/share/dbus-1/system-services"
install_dir "$PREFIX/share/polkit-1/actions"
install_dir "$PREFIX/share/applications"
install_dir "$PREFIX/lib/xfce4/panel/plugins"
install_dir "$PREFIX/share/xfce4/panel/plugins"

install_file "$RELEASE_DIR/netcore" "$PREFIX/bin/netcore" 755
install_file "$RELEASE_DIR/netcore-gui" "$PREFIX/bin/netcore-gui" 755
install_file "$RELEASE_DIR/netcore-daemon" "$PREFIX/sbin/netcore-daemon" 755
install_file packaging/freebsd/netcore.rc "$PREFIX/etc/rc.d/netcore" 755
install_file packaging/freebsd/org.netcore.NetworkManager.conf "$PREFIX/etc/dbus-1/system.d/org.netcore.NetworkManager.conf" 644
install_file packaging/freebsd/org.netcore.NetworkManager.service "$PREFIX/share/dbus-1/system-services/org.netcore.NetworkManager.service" 644
install_file packaging/freebsd/org.netcore.policy "$PREFIX/share/polkit-1/actions/org.netcore.policy" 644
install_file packaging/freebsd/netcore-gui.desktop "$PREFIX/share/applications/org.netcore.NetworkManager.Gui.desktop" 644
install_file "$RELEASE_DIR/libnetcore_xfce_plugin.so" "$PREFIX/lib/xfce4/panel/plugins/libnetcore_xfce_plugin.so" 755
install_file plugins/xfce/netcore-xfce-plugin/netcore-plugin.desktop "$PREFIX/share/xfce4/panel/plugins/netcore-plugin.desktop" 644

if [ -d plugins/gnome/netcore@freebsd.local ]; then
    info "安装 GNOME Shell 扩展"
    install_dir "$PREFIX/share/gnome-shell/extensions/netcore@freebsd.local"
    for file in plugins/gnome/netcore@freebsd.local/*; do
        [ -f "$file" ] || continue
        install -m 644 "$file"             "$DESTDIR$PREFIX/share/gnome-shell/extensions/netcore@freebsd.local/"
    done
fi

if [ "$INSTALL_KDE" = yes ]; then
    if [ -s "$KDE_PLUGIN" ]; then
        info "安装 KDE Plasma 集成"
        install_dir "$PREFIX/lib/qt6/qml/Netcore/Backend"
        install_file "$KDE_PLUGIN" "$PREFIX/lib/qt6/qml/Netcore/Backend/libnetcorebackendplugin.so" 755
        install_file "$KDE_DIR/qml/qmldir" "$PREFIX/lib/qt6/qml/Netcore/Backend/qmldir" 644
        install_file "$KDE_DIR/qml/netcorebackend.qmltypes" "$PREFIX/lib/qt6/qml/Netcore/Backend/netcorebackend.qmltypes" 644
        install_dir "$PREFIX/share/plasma/plasmoids/org.netcore.networkmanager/contents/ui"
        install_file "$KDE_DIR/metadata.json" "$PREFIX/share/plasma/plasmoids/org.netcore.networkmanager/metadata.json" 644
        install_file "$KDE_DIR/contents/ui/main.qml" "$PREFIX/share/plasma/plasmoids/org.netcore.networkmanager/contents/ui/main.qml" 644
        install_file "$KDE_DIR/contents/ui/CompactRepresentation.qml" "$PREFIX/share/plasma/plasmoids/org.netcore.networkmanager/contents/ui/CompactRepresentation.qml" 644
        install_file "$KDE_DIR/contents/ui/FullRepresentation.qml" "$PREFIX/share/plasma/plasmoids/org.netcore.networkmanager/contents/ui/FullRepresentation.qml" 644
        install_file "$KDE_DIR/contents/ui/PasswordDialog.qml" "$PREFIX/share/plasma/plasmoids/org.netcore.networkmanager/contents/ui/PasswordDialog.qml" 644
    else
        warn "KDE 预编译插件缺失，跳过 KDE Plasma 集成。"
    fi
else
    warn "未安装 KDE Plasma 集成（构建环境缺失或未选择）。"
fi

echo
echo "安装完成：${DESTDIR:-}$PREFIX（$MODE 模式）"
echo "下一步："
echo "  1. sysrc netcore_enable=YES"
echo "  2. service dbus restart   # 或重启，加载 D-Bus policy"
echo "  3. service netcore start"
echo "  4. netcore status"
echo "开发会话："
echo "  netcore-daemon --bus session --auth none &"
echo "  NETCORE_BUS=session netcore-gui"