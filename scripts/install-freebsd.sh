#!/bin/sh
set -eu

# netcore FreeBSD installer.
#
# Modes:
#   auto   detect a complete build environment; build when possible, otherwise
#          use prebuilt artifacts.
#   source force a source build.
#   binary install already-built artifacts; never invoke a compiler.
#
# Desktop integrations are selected separately. In a terminal, the installer
# shows a numbered menu; non-interactive runs auto-select the current desktop.
#
# PREFIX and DESTDIR may also be supplied through the environment.

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
cd "$ROOT"

MODE=auto
MODE_SET=no
CHECK_ONLY=no
YES=no
PREFIX="${PREFIX:-/usr/local}"
DESTDIR="${DESTDIR:-}"
CARGO="${CARGO:-cargo}"
CC="${CC:-cc}"
CXX="${CXX:-c++}"
DESKTOP_SPEC=""
DESKTOP_SPEC_GIVEN=no

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

INSTALL_XFCE=no
INSTALL_GNOME=no
INSTALL_KDE=no

CORE_BUILDABLE=no
KDE_BUILDABLE=no
MISSING_CORE=""
MISSING_KDE=""
PREBUILT_MISSING=""

AVAIL_XFCE=no
AVAIL_GNOME=no
AVAIL_KDE=no
CURRENT_DESKTOP=""
SELECTION_LABEL="无"

usage() {
    cat <<'EOF'
netcore FreeBSD 一键安装

用法：
  sudo ./scripts/install-freebsd.sh [选项]            # 自动检测/编译安装
  sudo ./scripts/install-freebsd-prebuilt.sh [选项]   # 直接安装已编译产物

安装模式：
  --auto              自动检测编译环境：可编译则编译，否则安装已有产物（默认）
  --source, --build   强制从源码编译安装；编译环境不完整即失败
  --binary, --prebuilt 只安装已经编译好的产物；不会探测或调用编译器

桌面集成（默认：终端中弹出序号菜单；非交互时选择当前桌面）：
  --desktop LIST      直接指定，不弹菜单：
                        auto         自动选择当前桌面（无可识别桌面时选择唯一已安装桌面）
                        all          安装 XFCE + GNOME + KDE 全部集成
                        none, 0      不安装任何桌面插件
                        xfce, 1      XFCE Panel 插件
                        gnome, 2     GNOME Shell 扩展
                        kde, 3       KDE Plasma 6 小部件
                        1,3          可组合多个序号或名称
  -y, --yes           非交互选择当前桌面，不显示菜单

其他：
  --check             只检测环境、产物和自动选择，不做编译或安装
  --prefix PATH       安装前缀，默认 /usr/local
  --destdir PATH      暂存根目录，适合打包或非 root 安装
  -h, --help          显示本帮助

环境变量：
  PREFIX、DESTDIR、CARGO、CARGO_TARGET_DIR、CC、CXX

编译环境检测：
  Rust/GTK 构建始终需要 cargo、rustc、C 连接器和 GTK4 开发文件。
  只有选择 XFCE 时才要求 GTK3 与 libxfce4panel；只有选择 KDE 时才要求 Qt6。
  自动模式在编译环境不完整时会检查预编译产物，两者都不可用则报错。

预编译产物：
  target/release/netcore
  target/release/netcore-gui
  target/release/netcore-daemon
  target/release/libnetcore_xfce_plugin.so（仅选择 XFCE 时需要）
  plugins/kde/netcore/build/Netcore/Backend/libnetcorebackendplugin.so
  或 target/kde-qml/Netcore/Backend/libnetcorebackendplugin.so（仅选择 KDE 时需要）

示例：
  sudo ./scripts/install-freebsd.sh
  sudo ./scripts/install-freebsd.sh --desktop 1
  sudo ./scripts/install-freebsd.sh --desktop xfce,gnome
  sudo ./scripts/install-freebsd-prebuilt.sh --desktop 2
  ./scripts/install-freebsd.sh --check --desktop all
  DESTDIR=/tmp/netcore-stage ./scripts/install-freebsd.sh --desktop none
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
        -y|--yes)
            YES=yes
            ;;
        --desktop)
            [ "$#" -ge 2 ] || die "--desktop 需要一个值"
            DESKTOP_SPEC="$2"
            DESKTOP_SPEC_GIVEN=yes
            shift
            ;;
        --desktop=*)
            DESKTOP_SPEC="${1#*=}"
            DESKTOP_SPEC_GIVEN=yes
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

detect_desktops() {
    AVAIL_XFCE=no
    AVAIL_GNOME=no
    AVAIL_KDE=no
    CURRENT_DESKTOP=""

    command -v xfce4-panel >/dev/null 2>&1 && AVAIL_XFCE=yes
    command -v gnome-shell >/dev/null 2>&1 && AVAIL_GNOME=yes
    command -v plasmashell >/dev/null 2>&1 && AVAIL_KDE=yes

    desktop_hint="${XDG_CURRENT_DESKTOP:-} ${DESKTOP_SESSION:-} ${GDMSESSION:-}"
    case "$desktop_hint" in
        *XFCE*|*xfce*|*Xfce*) CURRENT_DESKTOP=xfce; AVAIL_XFCE=yes ;;
        *GNOME*|*gnome*|*Gnome*) CURRENT_DESKTOP=gnome; AVAIL_GNOME=yes ;;
        *KDE*|*kde*|*Plasma*|*plasma*) CURRENT_DESKTOP=kde; AVAIL_KDE=yes ;;
    esac
}

any_desktop_selected() {
    [ "$INSTALL_XFCE" = yes ] || [ "$INSTALL_GNOME" = yes ] || [ "$INSTALL_KDE" = yes ]
}

set_selection_label() {
    label=""
    [ "$INSTALL_XFCE" = yes ] && label="XFCE"
    if [ "$INSTALL_GNOME" = yes ]; then
        [ -n "$label" ] && label="$label, "
        label="${label}GNOME"
    fi
    if [ "$INSTALL_KDE" = yes ]; then
        [ -n "$label" ] && label="$label, "
        label="${label}KDE"
    fi
    [ -n "$label" ] || label="无"
    SELECTION_LABEL="$label"
}

# Parse a comma/space separated list of names or numbers. 0/none and
# all/a are accepted only as the complete value.
parse_desktop_spec() {
    spec="$1"
    INSTALL_XFCE=no
    INSTALL_GNOME=no
    INSTALL_KDE=no

    case "$spec" in
        all|a)
            INSTALL_XFCE=yes
            INSTALL_GNOME=yes
            INSTALL_KDE=yes
            return 0
            ;;
        none|0)
            return 0
            ;;
    esac

    old_ifs="$IFS"
    IFS=', '
    # Word splitting here is intentional: the value is a comma/space list.
    set -- $spec
    IFS="$old_ifs"
    [ "$#" -gt 0 ] || return 1

    valid=1
    for token in "$@"; do
        case "$token" in
            1|xfce|XFCE) INSTALL_XFCE=yes ;;
            2|gnome|GNOME) INSTALL_GNOME=yes ;;
            3|kde|KDE) INSTALL_KDE=yes ;;
            *)
                valid=0
                break
                ;;
        esac
    done
    [ "$valid" -eq 1 ]
}

auto_desktop_spec() {
    if [ -n "$CURRENT_DESKTOP" ]; then
        printf '%s' "$CURRENT_DESKTOP"
        return 0
    fi
    count=0
    one=""
    if [ "$AVAIL_XFCE" = yes ]; then count=$((count + 1)); one=xfce; fi
    if [ "$AVAIL_GNOME" = yes ]; then count=$((count + 1)); one=gnome; fi
    if [ "$AVAIL_KDE" = yes ]; then count=$((count + 1)); one=kde; fi
    if [ "$count" -eq 1 ]; then
        printf '%s' "$one"
    else
        printf '%s' none
    fi
}

desktop_default_number() {
    if [ -n "$CURRENT_DESKTOP" ]; then
        case "$CURRENT_DESKTOP" in
            xfce) printf '%s' 1 ;;
            gnome) printf '%s' 2 ;;
            kde) printf '%s' 3 ;;
        esac
        return 0
    fi
    count=0
    one="0"
    if [ "$AVAIL_XFCE" = yes ]; then count=$((count + 1)); one=1; fi
    if [ "$AVAIL_GNOME" = yes ]; then count=$((count + 1)); one=2; fi
    if [ "$AVAIL_KDE" = yes ]; then count=$((count + 1)); one=3; fi
    if [ "$count" -eq 1 ]; then
        printf '%s' "$one"
    else
        printf '%s' 0
    fi
}

menu_tag() {
    if [ "$1" = yes ] && [ "$CURRENT_DESKTOP" = "$2" ]; then
        printf '%s' "[当前桌面]"
    elif [ "$1" = yes ]; then
        printf '%s' "[已安装]"
    else
        printf '%s' "[未检测到]"
    fi
}

interactive_desktop_menu() {
    default=$(desktop_default_number)
    case "$default" in
        1) default_text="1 (XFCE)" ;;
        2) default_text="2 (GNOME)" ;;
        3) default_text="3 (KDE)" ;;
        *) default_text="0 (不安装)" ;;
    esac
    while :; do
        printf '\n检测到桌面：\n'
        printf '  1) XFCE Panel 插件     %s\n' "$(menu_tag "$AVAIL_XFCE" xfce)"
        printf '  2) GNOME Shell 扩展    %s\n' "$(menu_tag "$AVAIL_GNOME" gnome)"
        printf '  3) KDE Plasma 小部件   %s\n' "$(menu_tag "$AVAIL_KDE" kde)"
        printf '  a) 全部安装\n'
        printf '  0) 不安装桌面插件\n'
        printf '请输入序号（可多选，例如 1 或 1,3）[%s]: ' "$default_text"
        if IFS= read -r answer; then :; else answer=""; fi
        [ -n "$answer" ] || answer="$default"
        case "$answer" in
            a|all)
                parse_desktop_spec all
                break
                ;;
            0|none)
                parse_desktop_spec none
                break
                ;;
        esac
        if parse_desktop_spec "$answer" && { any_desktop_selected || [ "$answer" = "0" ]; }; then
            break
        fi
        warn "无效选择：$answer"
    done
}

resolve_desktop_choice() {
    if [ "$DESKTOP_SPEC_GIVEN" = yes ]; then
        case "$DESKTOP_SPEC" in
            auto)
                spec=$(auto_desktop_spec)
                if [ "$spec" = none ]; then
                    warn "无法自动判断桌面环境；本次只安装核心组件（可用 --desktop all 安装全部插件）。"
                fi
                parse_desktop_spec "$spec"
                ;;
            *)
                parse_desktop_spec "$DESKTOP_SPEC" || die "无效的 --desktop 值：$DESKTOP_SPEC"
                ;;
        esac
    elif [ "$CHECK_ONLY" = yes ] || [ "$YES" = yes ] || [ ! -t 0 ]; then
        spec=$(auto_desktop_spec)
        if [ "$spec" = none ]; then
            warn "非交互模式无法自动判断桌面环境；本次只安装核心组件（可用 --desktop all 安装全部插件）。"
        fi
        parse_desktop_spec "$spec"
    else
        interactive_desktop_menu
    fi
    set_selection_label
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
        if ! pkg-config --exists gtk4 >/dev/null 2>&1; then
            MISSING_CORE="$MISSING_CORE pkg-config:gtk4"
        fi
        if [ "$INSTALL_XFCE" = yes ]; then
            for package in gtk+-3.0 libxfce4panel-2.0; do
                if ! pkg-config --exists "$package" >/dev/null 2>&1; then
                    MISSING_CORE="$MISSING_CORE pkg-config:$package"
                fi
            done
        fi
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
    command -v qmake6 >/dev/null 2>&1 && return 0
    [ -x /usr/local/lib/qt6/bin/qmake6 ] && return 0
    command -v qmake >/dev/null 2>&1 && return 0
    return 1
}

find_moc() {
    command -v moc >/dev/null 2>&1 && return 0
    for qmake in qmake6 /usr/local/lib/qt6/bin/qmake6 qmake; do
        command -v "$qmake" >/dev/null 2>&1 || continue
        libexec=$("$qmake" -query QT_INSTALL_LIBEXECS 2>/dev/null || true)
        if [ -n "$libexec" ] && [ -x "$libexec/moc" ]; then
            return 0
        fi
    done
    [ -x /usr/local/libexec/qt6/moc ] && return 0
    [ -x /usr/local/lib/qt6/libexec/moc ] && return 0
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
    for artifact in         "$RELEASE_DIR/netcore"         "$RELEASE_DIR/netcore-gui"         "$RELEASE_DIR/netcore-daemon"
    do
        if [ ! -s "$artifact" ]; then
            PREBUILT_MISSING="$PREBUILT_MISSING $artifact"
        fi
    done
    if [ "$INSTALL_XFCE" = yes ] && [ ! -s "$RELEASE_DIR/libnetcore_xfce_plugin.so" ]; then
        PREBUILT_MISSING="$PREBUILT_MISSING $RELEASE_DIR/libnetcore_xfce_plugin.so"
    fi
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
    set -- build --release --locked         -p netcore-daemon -p netcore-cli -p netcore-gui
    if [ "$INSTALL_XFCE" = yes ]; then
        set -- "$@" -p netcore-xfce-plugin
    fi
    info "构建 Rust 组件（桌面集成：$SELECTION_LABEL）"
    "$CARGO" "$@"
}

build_kde() {
    [ "$INSTALL_KDE" = yes ] || return 0

    if [ "$KDE_BUILDABLE" != yes ]; then
        if [ "$MODE" = auto ] && kde_prebuilt_available; then
            select_prebuilt_kde
            warn "Qt6 构建环境不完整，使用已有 KDE 预编译插件：$KDE_PLUGIN"
            return 0
        fi
        die "已选择 KDE 集成，但 Qt6 构建环境不完整（$MISSING_KDE），且没有可用的预编译插件。"
    fi

    info "构建 KDE Plasma QML 插件"
    if (cd "$KDE_DIR" && BUILD_DIR="$KDE_BUILD_DIR" ./build.sh --build); then
        if [ -s "$KDE_BUILT_PLUGIN" ]; then
            KDE_PLUGIN="$KDE_BUILT_PLUGIN"
            return 0
        fi
        warn "KDE 插件构建完成但未找到产物：$KDE_BUILT_PLUGIN"
    else
        warn "KDE 插件构建失败。"
    fi

    if [ "$MODE" = auto ] && kde_prebuilt_available; then
        select_prebuilt_kde
        warn "改用已有 KDE 预编译插件：$KDE_PLUGIN"
        return 0
    fi
    die "已选择 KDE 集成，但 KDE 插件构建失败且没有可用的预编译插件。"
}

require_prebuilt_kde() {
    [ "$INSTALL_KDE" = yes ] || return 0
    if ! kde_prebuilt_available; then
        die "已选择 KDE 集成，但没有预编译 QML 插件。
请安装 Qt6 开发环境并运行 --auto，或提供 target/kde-qml 产物。"
    fi
    select_prebuilt_kde
}

check_artifact_set() {
    if ! check_prebuilt_core; then
        die "缺少预编译核心产物：$PREBUILT_MISSING
请安装编译环境后运行 --source，或先构建对应组件。"
    fi
    if [ "$INSTALL_GNOME" = yes ] && [ ! -d plugins/gnome/netcore@freebsd.local ]; then
        die "已选择 GNOME 集成，但仓库中缺少 plugins/gnome/netcore@freebsd.local。"
    fi
    if [ "$INSTALL_KDE" = yes ] && [ ! -s "$KDE_PLUGIN" ]; then
        die "已选择 KDE 集成，但 KDE 插件产物不可用。"
    fi
}

# Root is needed only for direct installation without DESTDIR.
if [ "$CHECK_ONLY" = no ] && [ -z "$DESTDIR" ] && [ "$(id -u)" -ne 0 ]; then
    die "安装到 $PREFIX 需要 root 权限。请使用 sudo，或设置 DESTDIR 进行暂存安装。"
fi

if [ "$PREFIX" != "/usr/local" ]; then
    warn "服务文件和 GUI 桌面文件按 /usr/local 编写；安装到 $PREFIX 后需自行调整 Exec 路径。"
fi

detect_desktops
resolve_desktop_choice
info "桌面集成选择：$SELECTION_LABEL"

if [ "$MODE" != binary ]; then
    detect_core_env
    if [ "$INSTALL_KDE" = yes ]; then
        detect_kde_env
    fi
fi
check_prebuilt_core || true

if [ "$CHECK_ONLY" = yes ]; then
    printf '%s\n' "netcore 安装环境检测"
    printf '  安装前缀: %s\n' "$PREFIX"
    printf '  暂存根目录: %s\n' "${DESTDIR:-（未设置，直接安装）}"
    printf '  Cargo 目标目录: %s\n' "$TARGET_DIR"
    printf '  桌面集成: %s\n' "$SELECTION_LABEL"
    if [ "$MODE" = binary ]; then
        printf '%s\n' "  编译环境: 预编译模式，跳过探测"
    else
        if [ "$CORE_BUILDABLE" = yes ]; then
            printf '%s\n' "  Rust/GTK 编译环境: 完整"
        else
            printf '%s\n' "  Rust/GTK 编译环境: 不完整（$MISSING_CORE）"
        fi
        if [ "$INSTALL_KDE" = yes ]; then
            if [ "$KDE_BUILDABLE" = yes ]; then
                printf '%s\n' "  KDE 构建环境: 完整"
            else
                printf '%s\n' "  KDE 构建环境: 不完整（$MISSING_KDE）"
            fi
        else
            printf '%s\n' "  KDE 构建环境: 未选择"
        fi
    fi
    if [ -z "$PREBUILT_MISSING" ]; then
        printf '%s\n' "  预编译核心产物: 完整"
    else
        printf '%s\n' "  预编译核心产物: 缺失（$PREBUILT_MISSING）"
    fi
    if [ "$INSTALL_KDE" = yes ]; then
        if kde_prebuilt_available; then
            printf '%s\n' "  预编译 KDE 插件: 存在"
        else
            printf '%s\n' "  预编译 KDE 插件: 缺失"
        fi
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

case "$MODE" in
    source)
        if [ "$CORE_BUILDABLE" != yes ]; then
            die "编译环境不完整，缺少:$MISSING_CORE"
        fi
        build_core
        build_kde
        ;;
    binary)
        require_prebuilt_kde
        check_artifact_set
        ;;
    auto)
        if [ "$CORE_BUILDABLE" = yes ]; then
            info "检测到完整 Rust/GTK 编译环境，选择源码构建。"
            build_core
            build_kde
        elif [ -z "$PREBUILT_MISSING" ]; then
            warn "编译环境不完整（缺少:$MISSING_CORE），回退到预编译产物。"
            require_prebuilt_kde
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
install_dir "$PREFIX/share/icons/hicolor/scalable/apps"

install_file "$RELEASE_DIR/netcore" "$PREFIX/bin/netcore" 755
install_file "$RELEASE_DIR/netcore-gui" "$PREFIX/bin/netcore-gui" 755
install_file "$RELEASE_DIR/netcore-daemon" "$PREFIX/sbin/netcore-daemon" 755
install_file packaging/freebsd/netcore.rc "$PREFIX/etc/rc.d/netcore" 755
install_file packaging/freebsd/org.netcore.NetworkManager.conf "$PREFIX/etc/dbus-1/system.d/org.netcore.NetworkManager.conf" 644
install_file packaging/freebsd/org.netcore.NetworkManager.service "$PREFIX/share/dbus-1/system-services/org.netcore.NetworkManager.service" 644
install_file packaging/freebsd/org.netcore.policy "$PREFIX/share/polkit-1/actions/org.netcore.policy" 644
install_file packaging/freebsd/netcore-gui.desktop "$PREFIX/share/applications/org.netcore.NetworkManager.Gui.desktop" 644
install_file packaging/freebsd/org.netcore.NetworkManager.Gui.svg "$PREFIX/share/icons/hicolor/scalable/apps/org.netcore.NetworkManager.Gui.svg" 644

if [ "$INSTALL_XFCE" = yes ]; then
    info "安装 XFCE Panel 插件"
    install_dir "$PREFIX/lib/xfce4/panel/plugins"
    install_dir "$PREFIX/share/xfce4/panel/plugins"
    install_file "$RELEASE_DIR/libnetcore_xfce_plugin.so" "$PREFIX/lib/xfce4/panel/plugins/libnetcore_xfce_plugin.so" 755
    install_file plugins/xfce/netcore-xfce-plugin/netcore-plugin.desktop "$PREFIX/share/xfce4/panel/plugins/netcore-plugin.desktop" 644
fi

if [ "$INSTALL_GNOME" = yes ]; then
    info "安装 GNOME Shell 扩展"
    install_dir "$PREFIX/share/gnome-shell/extensions/netcore@freebsd.local"
    for file in plugins/gnome/netcore@freebsd.local/*; do
        [ -f "$file" ] || continue
        install -m 644 "$file"             "$DESTDIR$PREFIX/share/gnome-shell/extensions/netcore@freebsd.local/"
    done
fi

if [ "$INSTALL_KDE" = yes ]; then
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
fi

echo
echo "安装完成：${DESTDIR:-}$PREFIX（$MODE 模式）"
echo "桌面集成：$SELECTION_LABEL"
echo "下一步："
echo "  1. sysrc netcore_enable=YES"
echo "  2. 重启系统：reboot"
echo "     !! 不要重启系统 D-Bus 服务；它会终止当前桌面会话，导致桌面不可用。"
echo "  3. 重启后按需启动：service netcore start"
echo "  4. netcore status"
echo "开发会话："
echo "  netcore-daemon --bus session --auth none &"
echo "  NETCORE_BUS=session netcore-gui"