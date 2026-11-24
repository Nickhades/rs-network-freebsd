# netcore FreeBSD port skeleton.
#
# This file is provided for packaging work but has not been submitted to the
# Ports tree and cannot be validated without Mk/Uses/cargo.mk. Replace the
# GH_ACCOUNT/GH_PROJECT values (or set MASTER_SITES) before building.

PORTNAME=	netcore
DISTVERSION=	0.3.3
CATEGORIES=	net-mgmt
MASTER_SITES=	# local submission / GH
PKGNAMESUFFIX=	-network-manager

MAINTAINER=	netcore@example.invalid
COMMENT=	Native network manager for FreeBSD
WWW=		https://example.invalid/netcore

LICENSE=	BSD2CLAUSE
LICENSE_FILE=	${WRKSRC}/LICENSE

USES=		cargo gl pkgconfig qt:6
USE_GL=		gl
USE_GNOME=	gtk30 gtk40
USE_QT=		base declarative

LIB_DEPENDS=	libxfce4panel-2.0.so.4:x11-wm/xfce4-panel

# Runtime needs a D-Bus-capable wpa_supplicant and the dhcpcd backend.
# wpa_supplicant-devel is a ports package; do not fall back to the base
# wpa_supplicant, which may lack fi.w1.wpa_supplicant1 D-Bus support.
RUN_DEPENDS=	${LOCALBASE}/bin/dbus-daemon:devel/dbus \
		${LOCALBASE}/sbin/dhcpcd:net/dhcpcd \
		${LOCALBASE}/sbin/wpa_supplicant:security/wpa_supplicant-devel

# Filled from the repository Cargo.lock. Regenerate with:
#   cargo metadata --format-version 1 | ... (see PORTING.md)
CARGO_CRATES=			aho-corasick-1.1.5 \
		anstream-1.0.0 \
		anstyle-1.0.14 \ \
		anstyle-parse-1.0.0 \
		anstyle-query-1.1.5 \
		anstyle-wincon-3.0.11 \ \
		anyhow-1.0.105 \
		async-trait-0.1.92 \
		atk-0.19.0 \ \
		atk-sys-0.19.0 \
		autocfg-1.5.1 \
		bitflags-2.13.2 \ \
		bumpalo-3.20.4 \
		bytes-1.12.2 \
		cairo-rs-0.22.11 \ \
		cairo-sys-rs-0.22.9 \
		cfg-expr-0.20.11 \
		cfg-if-1.0.5 \ \
		clap-4.6.9 \
		clap_builder-4.6.7 \
		clap_derive-4.6.9 \ \
		clap_lex-1.1.2 \
		colorchoice-1.0.5 \
		endi-1.1.1 \ \
		equivalent-1.0.3 \
		errno-0.3.15 \
		fastrand-2.5.0 \ \
		field-offset-0.3.6 \
		futures-channel-0.3.34 \
		futures-core-0.3.34 \ \
		futures-executor-0.3.34 \
		futures-io-0.3.34 \
		futures-macro-0.3.34 \ \
		futures-task-0.3.34 \
		futures-util-0.3.34 \
		gdk-0.19.0 \ \
		gdk-pixbuf-0.22.11 \
		gdk-pixbuf-sys-0.22.9 \
		gdk-sys-0.19.0 \ \
		gdk4-0.11.5 \
		gdk4-sys-0.11.5 \
		getrandom-0.4.3 \ \
		gio-0.22.11 \
		gio-sys-0.22.11 \
		glib-0.22.11 \ \
		glib-macros-0.22.11 \
		glib-sys-0.22.11 \
		gobject-sys-0.22.9 \ \
		graphene-rs-0.22.8 \
		graphene-sys-0.22.9 \
		gsk4-0.11.5 \ \
		gsk4-sys-0.11.5 \
		gtk-0.19.0 \
		gtk-sys-0.19.0 \ \
		gtk3-macros-0.19.0 \
		gtk4-0.11.5 \
		gtk4-macros-0.11.5 \ \
		gtk4-sys-0.11.5 \
		hashbrown-0.17.2 \
		heck-0.5.0 \ \
		hex-0.4.3 \
		indexmap-2.14.2 \
		is_terminal_polyfill-1.70.2 \ \
		itoa-1.0.19 \
		js-sys-0.3.107 \
		lazy_static-1.5.1 \ \
		libc-0.2.191 \
		linux-raw-sys-0.12.1 \
		log-0.4.35 \ \
		matchers-0.2.0 \
		memchr-2.8.3 \
		memoffset-0.9.1 \ \
		mio-1.2.5 \
		nu-ansi-term-0.50.3 \
		once_cell-1.21.4 \ \
		once_cell_polyfill-1.70.2 \
		ordered-stream-0.2.0 \
		pango-0.22.9 \ \
		pango-sys-0.22.11 \
		pin-project-lite-0.2.18 \
		pkg-config-0.3.34 \ \
		proc-macro-crate-3.5.0 \
		proc-macro2-1.0.107 \
		quote-1.0.47 \ \
		r-efi-6.0.0 \
		regex-automata-0.4.19 \
		regex-syntax-0.8.12 \ \
		rpassword-7.5.5 \
		rtoolbox-0.0.6 \
		rustc_version-0.4.1 \ \
		rustix-1.1.6 \
		rustversion-1.0.23 \
		semver-1.0.28 \ \
		serde-1.0.229 \
		serde_core-1.0.229 \
		serde_derive-1.0.229 \ \
		serde_json-1.0.154 \
		serde_repr-0.1.21 \
		serde_spanned-1.1.3 \ \
		sharded-slab-0.1.7 \
		signal-hook-registry-1.4.8 \
		slab-0.4.12 \ \
		smallvec-1.16.6 \
		socket2-0.6.5 \
		strsim-0.11.1 \ \
		syn-2.0.119 \
		syn-3.0.10 \
		system-deps-9.0.1 \ \
		target-lexicon-0.13.5 \
		tempfile-3.27.0 \
		thread_local-1.1.10 \ \
		tokio-1.54.1 \
		tokio-macros-2.7.2 \
		toml-1.2.2+spec-1.1.0 \ \
		toml_datetime-1.1.3+spec-1.1.0 \
		toml_edit-0.25.20+spec-1.1.0 \
		toml_parser-1.1.6+spec-1.1.0 \ \
		toml_writer-1.2.0+spec-1.1.0 \
		tracing-0.1.44 \
		tracing-attributes-0.1.31 \ \
		tracing-core-0.1.36 \
		tracing-log-0.2.0 \
		tracing-subscriber-0.3.23 \ \
		uds_windows-1.2.1 \
		unicode-ident-1.0.27 \
		utf8parse-0.2.2 \ \
		uuid-1.29.3 \
		valuable-0.1.1 \
		version-compare-0.2.1 \ \
		wasi-0.11.1+wasi-snapshot-preview1 \
		wasm-bindgen-0.2.130 \
		wasm-bindgen-macro-0.2.130 \ \
		wasm-bindgen-macro-support-0.2.130 \
		wasm-bindgen-shared-0.2.130 \
		windows-link-0.2.1 \ \
		windows-sys-0.61.2 \
		winnow-1.0.5 \
		xfce-panel-0.1.0 \ \
		xfce-panel-sys-0.1.0 \
		zbus-6.5.0 \
		zbus_macros-6.5.0 \ \
		zbus_utils-6.1.0 \
		zcheapstr-1.4.0 \
		zmij-1.0.23 \ \
		zruntime-0.2.4
post-extract:
	@${REINPLACE_CMD} -e 's|/usr/local|${PREFIX}|g' \
		${WRKSRC}/packaging/freebsd/org.netcore.NetworkManager.service \
		${WRKSRC}/packaging/freebsd/netcore-gui.desktop \
		${WRKSRC}/packaging/freebsd/netcore.rc

post-build:
	(cd ${WRKSRC}/plugins/kde/netcore && QMAKE=${QMAKE} ./build.sh --build)

do-install:
	${MKDIR} ${STAGEDIR}${PREFIX}/bin
	${MKDIR} ${STAGEDIR}${PREFIX}/sbin
	${INSTALL_PROGRAM} ${WRKSRC}/target/release/netcore ${STAGEDIR}${PREFIX}/bin/
	${INSTALL_PROGRAM} ${WRKSRC}/target/release/netcore-gui ${STAGEDIR}${PREFIX}/bin/
	${INSTALL_PROGRAM} ${WRKSRC}/target/release/netcore-daemon ${STAGEDIR}${PREFIX}/sbin/
	${MKDIR} ${STAGEDIR}${PREFIX}/etc/rc.d
	${INSTALL_SCRIPT} ${WRKSRC}/packaging/freebsd/netcore.rc ${STAGEDIR}${PREFIX}/etc/rc.d/netcore
	${MKDIR} ${STAGEDIR}${PREFIX}/etc/dbus-1/system.d
	${INSTALL_DATA} ${WRKSRC}/packaging/freebsd/org.netcore.NetworkManager.conf ${STAGEDIR}${PREFIX}/etc/dbus-1/system.d/
	${MKDIR} ${STAGEDIR}${PREFIX}/share/dbus-1/system-services
	${INSTALL_DATA} ${WRKSRC}/packaging/freebsd/org.netcore.NetworkManager.service ${STAGEDIR}${PREFIX}/share/dbus-1/system-services/
	${MKDIR} ${STAGEDIR}${PREFIX}/share/polkit-1/actions
	${INSTALL_DATA} ${WRKSRC}/packaging/freebsd/org.netcore.policy ${STAGEDIR}${PREFIX}/share/polkit-1/actions/
	${MKDIR} ${STAGEDIR}${PREFIX}/share/applications
	${INSTALL_DATA} ${WRKSRC}/packaging/freebsd/netcore-gui.desktop ${STAGEDIR}${PREFIX}/share/applications/org.netcore.NetworkManager.Gui.desktop
	${MKDIR} ${STAGEDIR}${PREFIX}/share/icons/hicolor/scalable/apps
	${INSTALL_DATA} ${WRKSRC}/packaging/freebsd/org.netcore.NetworkManager.Gui.svg ${STAGEDIR}${PREFIX}/share/icons/hicolor/scalable/apps/
	${MKDIR} ${STAGEDIR}${PREFIX}/lib/xfce4/panel/plugins
	${INSTALL_LIB} ${WRKSRC}/target/release/libnetcore_xfce_plugin.so ${STAGEDIR}${PREFIX}/lib/xfce4/panel/plugins/
	${MKDIR} ${STAGEDIR}${PREFIX}/share/xfce4/panel/plugins
	${INSTALL_DATA} ${WRKSRC}/plugins/xfce/netcore-xfce-plugin/netcore-plugin.desktop ${STAGEDIR}${PREFIX}/share/xfce4/panel/plugins/
	${MKDIR} ${STAGEDIR}${QT_QMLDIR}/Netcore/Backend
	${INSTALL_LIB} ${WRKSRC}/plugins/kde/netcore/build/Netcore/Backend/libnetcorebackendplugin.so ${STAGEDIR}${QT_QMLDIR}/Netcore/Backend/
	${INSTALL_DATA} ${WRKSRC}/plugins/kde/netcore/qml/qmldir ${WRKSRC}/plugins/kde/netcore/qml/netcorebackend.qmltypes ${STAGEDIR}${QT_QMLDIR}/Netcore/Backend/
	${MKDIR} ${STAGEDIR}${PREFIX}/share/plasma/plasmoids/org.netcore.networkmanager/contents/ui
	${INSTALL_DATA} ${WRKSRC}/plugins/kde/netcore/metadata.json ${STAGEDIR}${PREFIX}/share/plasma/plasmoids/org.netcore.networkmanager/
	${MKDIR} ${STAGEDIR}${PREFIX}/share/gnome-shell/extensions/netcore@freebsd.local
	${INSTALL_DATA} ${WRKSRC}/plugins/gnome/netcore@freebsd.local/metadata.json \
		${WRKSRC}/plugins/gnome/netcore@freebsd.local/extension.js \
		${WRKSRC}/plugins/gnome/netcore@freebsd.local/indicator.js \
		${WRKSRC}/plugins/gnome/netcore@freebsd.local/menuItems.js \
		${WRKSRC}/plugins/gnome/netcore@freebsd.local/netcoreClient.js \
		${WRKSRC}/plugins/gnome/netcore@freebsd.local/panelTooltip.js \
		${WRKSRC}/plugins/gnome/netcore@freebsd.local/utils.js \
		${WRKSRC}/plugins/gnome/netcore@freebsd.local/stylesheet.css \
		${WRKSRC}/plugins/gnome/netcore@freebsd.local/README.md \
		${STAGEDIR}${PREFIX}/share/gnome-shell/extensions/netcore@freebsd.local/
	${INSTALL_DATA} ${WRKSRC}/plugins/kde/netcore/contents/ui/main.qml \
		${WRKSRC}/plugins/kde/netcore/contents/ui/CompactRepresentation.qml \
		${WRKSRC}/plugins/kde/netcore/contents/ui/FullRepresentation.qml \
		${WRKSRC}/plugins/kde/netcore/contents/ui/PasswordDialog.qml \
		${STAGEDIR}${PREFIX}/share/plasma/plasmoids/org.netcore.networkmanager/contents/ui/

.include <bsd.port.mk>