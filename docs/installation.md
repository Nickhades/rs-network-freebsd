# 安装

netcore 提供两种一键安装入口，并在安装前选择要集成的桌面：

| 入口 | 行为 | 需要编译器 |
|---|---|---|
| scripts/install-freebsd.sh | 自动检测编译环境；可编译则编译，否则尝试预编译产物 | 视环境而定 |
| scripts/install-freebsd-prebuilt.sh | 只安装已有编译产物，绝不探测或调用编译器 | 不需要 |

无论选择哪种模式，核心组件都会安装：守护进程、CLI、GTK4 界面、自绘
应用图标、rc.d、D-Bus service/policy、polkit 和桌面启动文件。桌面插件
只安装所选项。运行时依赖 ports 的 D-Bus 版 wpa_supplicant 和 dhcpcd：

    pkg install gtk4 dbus polkit dhcpcd wpa_supplicant-devel

安装脚本会检查 /usr/local/sbin/wpa_supplicant 的 -u/D-Bus 支持和 dhcpcd，
打印明确提示，但不会偷偷安装软件包。rc.conf 建议见 README 和 docs/freebsd.md：当前版本让系统 rc 启动
wpa_supplicant（ifconfig_wlan0="WPA"）和 dhcpcd，netcore 连接并沿用；
不要让系统只把 wlan0 up 而没有任何 wpa_supplicant。

## 选择桌面集成

在终端直接运行脚本时，会显示序号菜单：

    检测到桌面：
      1) XFCE Panel 插件     [当前桌面]
      2) GNOME Shell 扩展    [已安装]
      3) KDE Plasma 小部件   [已安装]
      a) 全部安装
      0) 不安装桌面插件
    请输入序号（可多选，例如 1 或 1,3）[1 (XFCE)]:

- 输入 1、2、3 选择单个插件。
- 输入 1,3 或 xfce,kde 选择多个。
- 输入 a 或 all 安装全部。
- 输入 0 或 none 只安装核心组件。
- 直接回车使用方括号中的默认项。

标签含义：

- [当前桌面]：XDG_CURRENT_DESKTOP / DESKTOP_SESSION / GDMSESSION 指向它。
- [已安装]：检测到对应桌面命令，但不是当前会话。
- [未检测到]：仍允许选择，以便为之后安装的桌面预装插件。

非交互运行（管道、DESTDIR 打包、无 TTY）默认选择当前桌面；如果无法判断
当前桌面，则只在恰好安装了一个受支持桌面时选择它，否则不安装桌面插件。
无人值守场景建议显式传入 --desktop：

    # 只安装 XFCE 插件
    sudo ./scripts/install-freebsd.sh --desktop 1
    sudo ./scripts/install-freebsd.sh --desktop xfce

    # 只安装 GNOME 扩展
    sudo ./scripts/install-freebsd.sh --desktop 2
    sudo ./scripts/install-freebsd.sh --desktop gnome

    # 只安装 KDE 小部件
    sudo ./scripts/install-freebsd.sh --desktop 3
    sudo ./scripts/install-freebsd.sh --desktop kde

    # 多选、全部、不选
    sudo ./scripts/install-freebsd.sh --desktop xfce,gnome
    sudo ./scripts/install-freebsd.sh --desktop all
    sudo ./scripts/install-freebsd.sh --desktop none

    # 自动选择当前桌面，不弹菜单
    sudo ./scripts/install-freebsd.sh --desktop auto
    sudo ./scripts/install-freebsd.sh --yes

只有在使用 --desktop all 或菜单选择 a 时才全量安装桌面插件。

## 自动模式

    sudo ./scripts/install-freebsd.sh

默认模式是 auto，会按桌面选择检测编译环境：

- 核心构建始终需要 cargo、rustc、C 连接器、pkg-config 和 GTK4 开发文件。
- 选择 XFCE 时额外要求 GTK3 与 libxfce4panel。
- 选择 KDE 时额外要求 Qt6（qmake6、moc、Qt6Core/DBus/Qml）。
- GNOME 扩展是纯 JS 数据文件，不需要编译。

行为：

1. 编译环境完整：
   - 构建核心 Rust 组件；
   - 如选择 XFCE，构建 XFCE 插件；
   - 如选择 KDE，构建 KDE QML 插件；
   - 安装核心组件和所选项。
2. 编译环境不完整：
   - 检查所选项对应的预编译产物；
   - 齐全则使用预编译安装；
   - 不齐全则打印缺失清单并退出，不做半安装。
3. 选择 KDE 但 Qt6 或预编译插件均不可用：直接失败，而不是静默漏装。
4. 未选择的桌面插件不会被检查、构建或安装。

KDE 构建目录使用 target/kde-qml，不写入源码树内的 build 目录，避免 root
安装后留下不可写的目录。

## 预编译模式

    sudo ./scripts/install-freebsd-prebuilt.sh --desktop 2

这个入口强制 --binary，只复制预编译产物和安装数据，不会探测或调用
cargo、qmake、moc、cc。所需产物取决于桌面选择：

    核心（始终）：
      target/release/netcore
      target/release/netcore-gui
      target/release/netcore-daemon

    选择 XFCE 时：
      target/release/libnetcore_xfce_plugin.so

    选择 KDE 时：
      plugins/kde/netcore/build/Netcore/Backend/libnetcorebackendplugin.so
      或 target/kde-qml/Netcore/Backend/libnetcorebackendplugin.so

    选择 GNOME 时：
      使用仓库中的 plugins/gnome/netcore@freebsd.local 数据，无需编译。

如果选择了某个集成但对应产物缺失，安装会失败并说明缺少什么；未选择的
集成完全不会检查。不再出现“全量安装但部分组件静默跳过”的行为。

在源码机上手工构建后使用预编译模式，可以避免 sudo 构建产生 root 所有的
target 文件：

    cargo build --release --locked \
      -p netcore-daemon -p netcore-cli -p netcore-gui
    cargo build --release --locked -p netcore-xfce-plugin   # 选择 XFCE 时
    (cd plugins/kde/netcore && ./build.sh --build)          # 选择 KDE 时
    sudo ./scripts/install-freebsd-prebuilt.sh --desktop all

注意：当前预编译模式依赖仓库中的安装数据文件和 target 产物，不是单独
自包含的二进制 tar 包。制作自包含发布包时，应把脚本、packaging/、
plugins/ 与所需 target 产物一同打包。

## 环境检查

不编译、不安装，只查看检测结果和自动模式决策：

    ./scripts/install-freebsd.sh --check --desktop all
    ./scripts/install-freebsd.sh --check --desktop 2

检查输出包含：

- 已选桌面集成；
- Rust/GTK 编译环境是否完整；
- KDE 构建环境（仅选择 KDE 时）；
- 所选集成需要的预编译产物；
- auto 模式将编译、回退还是失败。

--check 不会弹菜单，也不会要求 root。

## 强制模式

    sudo ./scripts/install-freebsd.sh --source --desktop xfce
    sudo ./scripts/install-freebsd.sh --binary --desktop kde

--source 不会回退到预编译产物；编译环境不完整会直接失败。
--binary 等价于预编译入口。
模式参数冲突（例如 --binary --source）会直接报错。

## 暂存安装与自定义前缀

设置 DESTDIR 可以非 root 安装到临时根目录，适合打包、jail 和 CI：

    DESTDIR=/tmp/netcore-stage ./scripts/install-freebsd.sh --desktop 1
    DESTDIR=/tmp/netcore-stage ./scripts/install-freebsd.sh --desktop all

任意 PREFIX：

    PREFIX=/opt/netcore DESTDIR=/tmp/netcore-stage \
      ./scripts/install-freebsd.sh --desktop gnome

服务文件、D-Bus service 与 GUI .desktop 的 Exec 路径按 /usr/local 编写；
使用其他 PREFIX 时需要同步修改，否则系统启动时会找错路径。

## 安装后

    sysrc netcore_enable=YES
    reboot

    # 重启后按需启动并检查：
    service netcore start
    service netcore status
    netcore status

为什么不能重启系统 D-Bus 服务：这会杀死依赖它的桌面会话和图形程序，通常会导致
当前桌面不可用。安装/更新 D-Bus policy 后请直接重启系统。

如果只是想更新二进制而不重启服务，替换文件后执行：

    service netcore restart

不要用重启 dbus 的方式加载 policy。

## 卸载

一键卸载会停止并禁用 netcore 服务，按 packaging/freebsd/pkg-plist 删除
核心组件、已安装的桌面插件、D-Bus/polkit/rc.d/桌面文件和自绘图标，并移除
rc.conf 中的 netcore_enable。

    sudo ./scripts/uninstall-freebsd.sh --yes

    # 或：
    sudo make uninstall UNINSTALL_ARGS=--yes

    # 只预览：
    ./scripts/uninstall-freebsd.sh --dry-run

日志默认保留；需要一并删除时加 --purge。脚本以仓库中的
packaging/freebsd/pkg-plist 作为文件清单，因此需在 checkout 内运行。只删除
netcore 自己的文件，不会清理共享目录中的其他软件文件；GNOME/KDE/XFCE 集成
目录只有在删空后才会 rmdir。

暂存/自定义前缀：

    DESTDIR=/tmp/netcore-stage PREFIX=/usr/local \
      ./scripts/uninstall-freebsd.sh --yes
    PREFIX=/opt/netcore ./scripts/uninstall-freebsd.sh --yes

设置 DESTDIR 时不会停止宿主服务、不会修改宿主 rc.conf。卸载完成后请重启
系统，使 D-Bus policy/service 和桌面面板插件完全清理；不要重启系统 D-Bus
服务。若通过 FreeBSD pkg 安装，应使用 pkg delete 而不是本脚本。

## 故障排查

- 看到 [未检测到] 但仍选择：允许安装；确保之后安装对应桌面。
- 自动模式打印“编译环境不完整”：按所选项安装 XFCE/Qt6 构建依赖，或使用
  预编译入口。
- 自动模式打印“预编译产物缺少”：在用户环境中构建所选组件，或改用 --source。
- 选择 KDE 但失败：安装 qt6-base、qt6-declarative、qt6-tools，或提供预编译
  QML 插件。
- 提示需要 root：直接安装 /usr/local 需要 sudo；也可以设置 DESTDIR 暂存。
- 无人值守却只装了核心：未传 --desktop，且没有可判断的当前桌面；请显式指定。
- 旧安装残留：使用 sudo ./scripts/uninstall-freebsd.sh --yes 一键清理；
  也可以先用 --dry-run 查看将删除的路径。