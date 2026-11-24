# 桌面集成

三种桌面扩展都直接使用 netcore 的 D-Bus 接口，不解析 CLI 输出，不需要
额外的守护进程或权限。它们共享同一份模型（docs/json-schema.md）。

## 通用前提

1. 系统总线上的守护进程运行：
       service netcore start
       gdbus call --system --dest org.netcore.NetworkManager \
         --object-path /org/netcore/NetworkManager \
         --method org.netcore.NetworkManager.GetSnapshot
2. 用户有权执行修改操作：root、wheel 组成员，或 polkit 交互认证。
3. 安装脚本按选择把扩展放到以下系统目录：
   - XFCE: /usr/local/lib/xfce4/panel/plugins + share/xfce4/panel/plugins
   - GNOME: /usr/local/share/gnome-shell/extensions/netcore@freebsd.local
   - KDE: /usr/local/share/plasma/plasmoids/org.netcore.networkmanager
     以及 /usr/local/lib/qt6/qml/Netcore/Backend

安装示例：

    sudo ./scripts/install-freebsd.sh --desktop 1
    sudo ./scripts/install-freebsd.sh --desktop xfce,gnome
    sudo ./scripts/install-freebsd.sh --desktop all

安装脚本提供桌面插件序号菜单，可只安装当前桌面对应项，或使用
--desktop all 安装三者。KDE 需要 Qt6 开发文件，XFCE 需要
libxfce4panel 开发文件，GNOME 扩展无需编译。

## 效果截图

- XFCE Panel：[面板](screenshot-xfce-panel.png)、[菜单](screenshot-xfce-menu.png)
- GNOME Shell：[扩展菜单](screenshot-gnome-extension.png)
- KDE Plasma：[小部件完整表示](screenshot-kde-plasmoid.png)

截图使用隔离 Xvfb + 私有 D-Bus + mock 后端，只展示布局；真实网络扫描见
docs/testing.md。

## XFCE Panel

实现：Rust cdylib，运行在 xfce4-panel 进程内。通过 GDBus 异步读取
GetSnapshot，订阅 SnapshotChanged，菜单在打开时重建。

安装后：

    xfce4-panel -q
    xfce4-panel &

右键面板 -> 面板 -> 添加新项目 -> “netcore Network”。面板显示 Wi-Fi
符号和当前 SSID，悬停显示状态/IP。菜单支持 Wi-Fi 开关、网络列表、连接、
断开设备、扫描、隐藏网络和打开 netcore-gui。

已在隔离 Xvfb + 私有 D-Bus 中验证插件加载，日志显示
new item (type=object-type, name=netcore-plugin)；截图使用 mock 数据。

详见 plugins/xfce/netcore-xfce-plugin/README.md。

## GNOME Shell

实现：GJS ESM 扩展，Gio.DBusConnection 异步调用，订阅 SnapshotChanged、
NameOwnerChanged，5 秒兜底刷新。密码框使用 St.PasswordEntry，不调用
CLI。支持 GNOME Shell 45–48。

安装后：

    gnome-extensions enable netcore@freebsd.local
    # X11: Alt+F2 -> r
    # Wayland: 注销并重新登录

系统目录安装对 GNOME Shell 可见，但若使用用户目录，请把整个
netcore@freebsd.local 复制到 ~/.local/share/gnome-shell/extensions/。

详见 plugins/gnome/netcore@freebsd.local/README.md。

## KDE Plasma 6

实现：QML Plasma Applet + Netcore.Backend 1.0 C++ QML 插件。后端使用
QDBusConnection::asyncCall + QDBusPendingCallWatcher，订阅信号，面板紧凑
表示只显示图标，完整表示显示 Wi-Fi/设备详情。需要 Qt6 与 Plasma 6 QML
运行时。

安装脚本调用插件自带的 build.sh（无 CMake 时也可用）。手动安装：

    cd plugins/kde/netcore
    ./build.sh --check
    sudo ./build.sh --install
    # 注销并重新登录 Plasma，或在 Plasma 会话中重启 plasmashell

然后在面板添加“netcore 网络管理器”。

详见 plugins/kde/netcore/README.md。

## 设计一致性

- 三者都使用同样的方法签名 (sssb)、(sb)、(s) 等，不依赖私有扩展。
- 都从 SnapshotChanged 更新；GUI 使用 3 秒轮询兜底，XFCE 使用 5 秒，
  GNOME 5 秒，KDE 15 秒。
- 都通过 D-Bus 错误名区分 ServiceUnknown、NotAuthorized、Timeout 和
  守护进程业务错误。
- 均不把密码写入磁盘或日志。
- 面板扩展不直接调用 ifconfig/dhclient，所有特权操作回到守护进程。