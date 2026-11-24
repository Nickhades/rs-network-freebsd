#!/bin/sh
set -eu

# netcore one-click uninstaller.
#
# Removes the files listed in packaging/freebsd/pkg-plist from PREFIX, stops
# the rc.d service, removes netcore_enable, and leaves unrelated files alone.
# DESTDIR can be used for staged tests without touching the host.

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
cd "$ROOT"

PREFIX="${PREFIX:-/usr/local}"
DESTDIR="${DESTDIR:-}"
YES=no
DRY_RUN=no
PURGE=no
MANIFEST="$ROOT/packaging/freebsd/pkg-plist"

usage() {
    cat <<'EOF'
netcore 一键卸载

用法：
  sudo ./scripts/uninstall-freebsd.sh [选项]
  ./scripts/uninstall-freebsd.sh --dry-run

选项：
  -y, --yes           不再询问确认
  --dry-run, --check  只显示将删除的文件，不执行删除
  --purge             同时删除 /var/log/netcore.log
  --prefix PATH       安装前缀，默认 /usr/local
  --destdir PATH      暂存根目录；设置后不会停止服务、不会修改 rc.conf
  -h, --help          显示本帮助

环境变量：
  PREFIX、DESTDIR

说明：
  卸载核心组件、已安装的 XFCE/GNOME/KDE 插件、桌面文件、D-Bus policy/
  service、polkit、rc.d 与自绘图标。删除 D-Bus policy 后请重启系统，不要
  重启系统 D-Bus 服务，否则会终止当前桌面会话。日志默认保留，--purge 删除。
EOF
}

info() {
    printf '==> %s\n' "$*"
}

warn() {
    printf '警告: %s\n' "$*" >&2
}

die() {
    printf '错误: %s\n' "$*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        -y|--yes)
            YES=yes
            ;;
        --dry-run|--check)
            DRY_RUN=yes
            ;;
        --purge)
            PURGE=yes
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

# Normalize only trailing slashes. Every removal below is scoped to TARGET.
PREFIX="${PREFIX%/}"
[ -n "$PREFIX" ] || die "PREFIX 不能为空"
case "$PREFIX" in
    /) die "拒绝使用 PREFIX=/ 卸载" ;;
    /*) ;;
    *) die "PREFIX 必须是绝对路径: $PREFIX" ;;
esac
case "$PREFIX/" in
    *"/../"*) die "PREFIX 不能包含 ..：$PREFIX" ;;
esac

if [ -n "$DESTDIR" ]; then
    [ "$DESTDIR" != "/" ] || die "拒绝使用 DESTDIR=/"
    DESTDIR="${DESTDIR%/}"
    case "$DESTDIR/" in
        *"/../"*) die "DESTDIR 不能包含 ..：$DESTDIR" ;;
    esac
fi
TARGET="$DESTDIR$PREFIX"

[ -f "$MANIFEST" ] || die "找不到安装清单: $MANIFEST"
[ -d "$TARGET" ] || info "目标目录不存在，可能已卸载: $TARGET"

if [ "$DRY_RUN" = no ] && [ -z "$DESTDIR" ] && [ "$(id -u)" -ne 0 ]; then
    die "卸载安装到 $PREFIX 的软件需要 root；可加 sudo，或设置 DESTDIR 进行暂存测试。"
fi

# Preflight: only touch the service/rc.conf if some netcore file is present.
found=0
while IFS= read -r relative || [ -n "$relative" ]; do
    [ -n "$relative" ] || continue
    case "$relative" in
        \#*|@*) continue ;;
    esac
    if [ -e "$TARGET/$relative" ] || [ -L "$TARGET/$relative" ]; then
        found=$((found + 1))
    fi
done < "$MANIFEST"

if [ "$found" -eq 0 ] && [ ! -x "$DESTDIR$PREFIX/etc/rc.d/netcore" ]; then
    info "未发现 netcore 安装文件：$TARGET"
    exit 0
fi

printf '%s\n' "即将卸载：$TARGET"
printf '%s\n' "将移除 netcore 核心组件、已安装的桌面插件、D-Bus/polkit/rc.d/桌面文件。"
if [ "$DRY_RUN" = no ] && [ -z "$DESTDIR" ]; then
    printf '%s\n' "将停止 netcore 服务，并从 rc.conf 移除 netcore_enable。"
fi
if [ "$PURGE" = yes ]; then
    printf '%s\n' "将删除日志 /var/log/netcore.log。"
fi
printf '%s\n' "删除后需要重启系统；不要重启系统 D-Bus 服务。"
if [ "$DRY_RUN" = no ] && [ "$YES" != yes ]; then
    printf '继续卸载? [y/N] '
    [ -t 0 ] || die "非交互卸载需要 --yes"
    IFS= read -r answer || answer=""
    case "$answer" in
        y|Y|yes|YES) ;;
        *) info "已取消"; exit 0 ;;
    esac
fi

if [ "$DRY_RUN" = no ] && [ -z "$DESTDIR" ]; then
    if [ -x "$PREFIX/etc/rc.d/netcore" ] && command -v service >/dev/null 2>&1; then
        info "停止 netcore 服务（如正在运行）"
        service netcore stop >/dev/null 2>&1 || true
    fi
    if command -v sysrc >/dev/null 2>&1; then
        sysrc -x netcore_enable >/dev/null 2>&1 || true
    fi
fi

removed=0
missing=0
while IFS= read -r relative; do
    [ -n "$relative" ] || continue
    case "$relative" in
        \#*|@*) continue ;;
        /*|*..*)
            warn "跳过安装清单中的不安全路径: $relative"
            continue
            ;;
    esac
    path="$TARGET/$relative"
    if [ -e "$path" ] || [ -L "$path" ]; then
        if [ "$DRY_RUN" = yes ]; then
            printf '将删除 %s\n' "$path"
        else
            rm -f "$path"
            printf '已删除 %s\n' "$path"
        fi
        removed=$((removed + 1))
    else
        missing=$((missing + 1))
    fi
done < "$MANIFEST"

if [ "$DRY_RUN" = no ]; then
    # Remove only our now-empty integration directories. rmdir refuses to touch
    # directories that contain unrelated user/package files.
    for directory in \
        "$TARGET/share/gnome-shell/extensions/netcore@freebsd.local" \
        "$TARGET/lib/qt6/qml/Netcore/Backend" \
        "$TARGET/lib/qt6/qml/Netcore" \
        "$TARGET/share/plasma/plasmoids/org.netcore.networkmanager/contents/ui" \
        "$TARGET/share/plasma/plasmoids/org.netcore.networkmanager/contents" \
        "$TARGET/share/plasma/plasmoids/org.netcore.networkmanager" \
        "$TARGET/share/xfce4/panel/plugins" \
        "$TARGET/lib/xfce4/panel/plugins" \
        "$TARGET/share/icons/hicolor/scalable/apps"
    do
        if [ -d "$directory" ]; then
            rmdir "$directory" 2>/dev/null || true
        fi
    done

    if [ -z "$DESTDIR" ]; then
        rm -f /var/run/netcore.pid
        if [ "$PURGE" = yes ]; then
            rm -f /var/log/netcore.log
        else
            info "日志保留：/var/log/netcore.log（--purge 可删除）"
        fi
        if command -v pgrep >/dev/null 2>&1 && pgrep -f "$PREFIX/sbin/netcore-daemon" >/dev/null 2>&1; then
            warn "仍有 netcore-daemon 进程在运行；请重启系统使其完全退出。"
        fi
    fi
fi

if [ "$DRY_RUN" = yes ]; then
    info "预览完成：未删除任何文件（已存在 $removed 个安装路径）。"
else
    info "卸载完成：已删除 $removed 个路径，$missing 个不存在。"
    if [ -n "$DESTDIR" ]; then
        info "这是 DESTDIR 暂存卸载，没有修改宿主服务或 rc.conf。"
    else
        info "请重启系统，使 D-Bus policy/service 与桌面插件完全清理。"
        info "不要重启系统 D-Bus 服务，否则会终止当前桌面会话。"
    fi
fi