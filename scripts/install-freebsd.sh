#!/bin/sh
set -eu

# Install netcore from a checkout. PREFIX is /usr/local by default. DESTDIR can
# point at a staging directory (package build, jail image).
PREFIX="${PREFIX:-/usr/local}"
DESTDIR="${DESTDIR:-}"
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

if [ "$PREFIX" != "/usr/local" ]; then
    echo "warning: service files and the GUI .desktop are written for /usr/local;" >&2
    echo "         override Exec paths if you install under $PREFIX." >&2
fi

echo "==> building release binaries"
cargo build --release -p netcore-daemon -p netcore-cli -p netcore-gui
echo "==> building XFCE plugin"
cargo build --release -p netcore-xfce-plugin

install_dir() { install -d "$DESTDIR$1"; }
install_file() { install -m "${3:-755}" "$1" "$DESTDIR$2"; }
install_dir "$PREFIX/bin"
install_dir "$PREFIX/sbin"
install_dir "$PREFIX/etc/rc.d"
install_dir "$PREFIX/etc/dbus-1/system.d"
install_dir "$PREFIX/share/dbus-1/system-services"
install_dir "$PREFIX/share/polkit-1/actions"
install_dir "$PREFIX/share/applications"
install_dir "$PREFIX/lib/xfce4/panel/plugins"
install_dir "$PREFIX/share/xfce4/panel/plugins"

install_file target/release/netcore "$PREFIX/bin/netcore" 755
install_file target/release/netcore-gui "$PREFIX/bin/netcore-gui" 755
install_file target/release/netcore-daemon "$PREFIX/sbin/netcore-daemon" 755
install_file packaging/freebsd/netcore.rc "$PREFIX/etc/rc.d/netcore" 755
install_file packaging/freebsd/org.netcore.NetworkManager.conf "$PREFIX/etc/dbus-1/system.d/org.netcore.NetworkManager.conf" 644
install_file packaging/freebsd/org.netcore.NetworkManager.service "$PREFIX/share/dbus-1/system-services/org.netcore.NetworkManager.service" 644
install_file packaging/freebsd/org.netcore.policy "$PREFIX/share/polkit-1/actions/org.netcore.policy" 644
install_file packaging/freebsd/netcore-gui.desktop "$PREFIX/share/applications/org.netcore.NetworkManager.Gui.desktop" 644
install_file target/release/libnetcore_xfce_plugin.so "$PREFIX/lib/xfce4/panel/plugins/libnetcore_xfce_plugin.so" 755
install_file plugins/xfce/netcore-xfce-plugin/netcore-plugin.desktop "$PREFIX/share/xfce4/panel/plugins/netcore-plugin.desktop" 644

if [ -d plugins/gnome/netcore@freebsd.local ]; then
    echo "==> installing GNOME Shell extension"
    install_dir "$PREFIX/share/gnome-shell/extensions/netcore@freebsd.local"
    for file in plugins/gnome/netcore@freebsd.local/*; do
        [ -f "$file" ] || continue
        install -m 644 "$file" \
            "$DESTDIR$PREFIX/share/gnome-shell/extensions/netcore@freebsd.local/"
    done
fi

if [ -d plugins/kde/netcore ]; then
    echo "==> building and installing KDE Plasma applet"
    (cd plugins/kde/netcore && \
        DESTDIR="$DESTDIR" PREFIX="$PREFIX" QML_DIR="$PREFIX/lib/qt6/qml" \
        ./build.sh --install)
fi

echo
echo "Installed under ${DESTDIR}$PREFIX"
echo "Next:"
echo "  1. sysrc netcore_enable=YES"
echo "  2. service dbus restart   # or reboot, to load the D-Bus policy"
echo "  3. service netcore start"
echo "  4. netcore status"
echo "For a desktop-only development session:"
echo "  netcore-daemon --bus session --auth none &"
echo "  NETCORE_BUS=session netcore-gui"