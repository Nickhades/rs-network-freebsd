# netcore — FreeBSD 原生网络管理器

netcore 是一个面向 FreeBSD 15.1 的原生网络管理器，采用 Rust 2024 编写。
核心目标是提供与 Linux NetworkManager 类似的使用体验，但使用 FreeBSD 的
系统原语：wpa_supplicant（D-Bus 或控制套接字）、dhclient(8)、
getifaddrs(3) 与 NETLINK_ROUTE，以及 D-Bus 作为稳定的控制面。

界面包括：

- 常驻的 root 守护进程 netcore-daemon
- 命令行客户端 netcore
- GTK4 桌面应用 netcore-gui
- XFCE Panel 插件（Rust，原生 GDBus）
- GNOME Shell 扩展（GJS）
- KDE Plasma 6 小部件（QML/C++）

![netcore GTK4 网络管理器 - 默认深色](docs/screenshot-gui.png)

![netcore GTK4 网络管理器 - 浅色主题](docs/screenshot-gui-light.png)

![XFCE Panel 插件](docs/screenshot-xfce-panel.png)

界面配色、字体、图标和加速检测说明见 [docs/ui.md](docs/ui.md)。

![netcore 品牌标志](docs/screenshot-brand.png)
SSID 真实性与跨系统设备发现见 [docs/authenticity.md](docs/authenticity.md)。

## 功能

- Wi-Fi 扫描、连接、断开，WPA/WPA2/WPA3-SAE 密码网络
- wpa_supplicant 自动探测：优先 fi.w1.wpa_supplicant1 D-Bus，缺失时回退
  到控制套接字；wpa_cli、ifconfig、dhclient 与控制目录按 PATH/环境/实际
  目录自动发现（FreeBSD ports 构建常常没有 D-Bus 后端）
- 连接成功后启动/接管 dhclient，断开时只停止自己启动的进程
- 监听 NETLINK_ROUTE 的链路与地址事件，并保留 getifaddrs 周期轮询兜底
- 设备状态、IP、MAC、MTU 与 Wi-Fi 信号/安全性统一建模
- Wi-Fi 接口通过 ifconfig -g wlan 动态识别，不依赖 wlanN 名称
- SSID 按原始字节保存，附带无损 ssid_hex；中文在 C locale 下仍正确显示
- 版本化 JSON-over-D-Bus 协议，方便 GJS、QML、shell 调用
- root + wheel 组 + polkit 三层授权；只读接口对所有本地用户开放
- 可选 Capsicum：Netlink 解析器运行在 cap_enter(2) 后的子进程中
- rc.d 服务、D-Bus 激活、polkit action、FreeBSD 安装脚本
- GTK4 界面固定深色/浅色两套配色，默认深色，不跟随系统桌面主题
- 自绘 Cairo 图标、窗口控制按钮和应用图标；主窗口左上角显示 netcore 字标，
  任务栏图标在运行时由自绘 PNG 设置，不依赖系统图标主题
- 自动识别系统字体族，统一粗体、抗锯齿与文本后处理
- 启动时检测 DRM GPU；无 GPU 时自动回退 Cairo CPU 渲染，并在界面底部展示

## 快速开始

依赖（FreeBSD ports）：

    pkg install gtk4 dbus polkit
    # 自动编译核心还需要：
    pkg install rust pkgconf
    # 选择 XFCE 集成还需要：
    pkg install xfce4-panel
    # 选择 KDE 集成还需要：
    pkg install qt6-base qt6-declarative qt6-tools
    # GNOME Shell 扩展无需编译依赖
    # dhclient(8) 与 wpa_supplicant 属于 FreeBSD 基本系统

安装时会先选择要安装的桌面集成：

    1) XFCE Panel 插件
    2) GNOME Shell 扩展
    3) KDE Plasma 6 小部件
    a) 全部   0) 不安装

也可以直接指定，不进入菜单：

    # 两种一键入口
    sudo ./scripts/install-freebsd.sh                         # 自动编译/安装
    sudo ./scripts/install-freebsd-prebuilt.sh                # 只安装已有产物

    # 只为当前桌面安装对应插件
    sudo ./scripts/install-freebsd.sh --desktop auto

    # 只安装 XFCE，或同时安装 XFCE 和 GNOME
    sudo ./scripts/install-freebsd.sh --desktop 1
    sudo ./scripts/install-freebsd.sh --desktop xfce,gnome

    # 预编译模式选择 GNOME；不会调用编译器
    sudo ./scripts/install-freebsd-prebuilt.sh --desktop 2

    # 只检测环境、产物和自动选择
    ./scripts/install-freebsd.sh --check --desktop all

脚本会安装核心组件（守护进程、CLI、GTK4 界面、rc.d、D-Bus、polkit），
再只安装所选桌面插件。默认在终端中弹出序号菜单；非交互运行或指定
--desktop auto 时，自动选择当前桌面。只有显式选择 all（菜单 a 或
--desktop all）才会全量安装。

自动编译环境也是按选择检测的：选择 XFCE 才要求 GTK3/libxfce4panel，
选择 KDE 才要求 Qt6。编译环境不完整时会尝试对应预编译产物；选中的集成
仍不可用则明确失败，不会静默漏装。

安装后：

    sysrc netcore_enable=YES
    service dbus restart
    service netcore start
    service netcore status
    netcore status

暂存或无人值守安装时请显式选择桌面：

    DESTDIR=/tmp/netcore-stage ./scripts/install-freebsd.sh --desktop 1
    DESTDIR=/tmp/netcore-stage ./scripts/install-freebsd.sh --desktop all

注意：服务文件按 /usr/local 前缀编写。安装到其他 PREFIX 时需要相应修改
D-Bus service 与 .desktop 的 Exec 路径。完整说明见
[docs/installation.md](docs/installation.md)。

## 开发会话（不需要 root 安装）

守护进程没有 root 时仍可完成扫描与状态展示，但修改网络会明确返回
NotAuthorized。适合开发 GUI/插件：

    cargo build
    ./target/debug/netcore-daemon --bus session --wpa-backend ctrl --auth none &
    NETCORE_BUS=session ./target/debug/netcore status
    NETCORE_BUS=session ./target/debug/netcore scan
    NETCORE_BUS=session ./target/debug/netcore-gui
    NETCORE_BUS=session ./target/debug/netcore-gui --theme light

生产环境绝不要使用 --auth none 或 session 总线。

## CLI

    netcore status
    netcore --json status
    netcore list
    netcore scan
    netcore connect --ssid "My Wi-Fi"
    netcore connect --ssid "My Wi-Fi" --password-stdin
    netcore disconnect
    netcore wifi off
    netcore monitor
    netcore doctor

密码默认从终端隐藏读取；脚本可以使用 --password-stdin。

## 架构

    +---------------------+        D-Bus (system)         +------------------+
    | netcore-gui (GTK4)  |<----------------------------->|                  |
    | netcore (CLI)       |                               |  netcore-daemon  |
    | XFCE / GNOME / KDE  |<--------- signals ------------|  (root)          |
    +---------------------+                               +--------+---------+
                                                                   |
                       +----------------------+--------------------+--------------------+
                       |                      |                    |                    |
              wpa_supplicant            dhclient(8)         NETLINK_ROUTE       getifaddrs(3)
              D-Bus / ctrl socket       per-interface       link/addr events    reconcile
                                                                   |
                                                           Capsicum helper (optional)

更详细的模块划分见 docs/architecture.md。

## D-Bus 接口

服务名 org.netcore.NetworkManager，对象路径 /org/netcore/NetworkManager，
接口 org.netcore.NetworkManager。所有复杂返回值为 JSON 字符串，签名简单，
适合多种语言。完整契约见 docs/dbus-api.md，数据模型见 docs/json-schema.md。

    gdbus call --system \
      --dest org.netcore.NetworkManager \
      --object-path /org/netcore/NetworkManager \
      --method org.netcore.NetworkManager.GetSnapshot

## 桌面集成

- XFCE：Rust 插件，见 plugins/xfce/netcore-xfce-plugin/README.md
- GNOME：GNOME Shell 扩展，见 plugins/gnome/netcore@freebsd.local/README.md
- KDE Plasma 6：QML 小部件，见 plugins/kde/netcore/README.md

三者都直接使用 netcore D-Bus 后端，不依赖 shell 解析。安装与使用说明见 docs/desktop-integration.md。

## 安全

- 守护进程以 root 运行，但只暴露必要的 D-Bus 方法。
- 系统总线上的修改操作按以下顺序检查：root、wheel 组（可配置）、polkit
  org.netcore.manage。
- 密码不写入磁盘，也不写日志。控制套接字后端通过临时 wpa_cli 参数传密码，
  会短暂出现在进程参数中；启用 wpa_supplicant D-Bus 后端时没有这一暴露。
- Capsicum 只隔离 Netlink 解析进程；主进程必须保留文件系统与 fork/exec
  能力来运行 dhclient 和访问 wpa_supplicant，不能整体 cap_enter。
- 详细威胁模型与加固建议见 docs/security.md。

## 已知限制

- 暂不支持 WPA-Enterprise（802.1X）与 WEP（后者出于安全原因主动拒绝）。
- 非 UTF-8 SSID 可完整扫描、显示并保留 ssid_hex，但字符串 ConnectWifi
  接口不会猜测原始字节；主 GUI 会阻止这类连接，而不是连到错误名称。
- 不持久化已保存网络；每次连接把网络临时加入 wpa_supplicant。
- 不主动探测互联网连通性；connectivity=local 表示已有本地地址。
- FreeBSD rtnetlink 仍在演进，NETLINK_ROUTE 只作为加速路径，轮询兜底
  保证最终一致性。
- 控制套接字后端的密码有短暂的 argv 暴露窗口（见安全说明）。
- 打包目录提供 port 骨架与本地安装脚本，尚未提交到 FreeBSD Ports 树。

## 工程结构

    crates/netcore-common     共享模型与协议常量
    crates/netcore-client     Rust D-Bus 客户端
    crates/netcore-daemon     守护进程、状态机、Wi-Fi/DHCP/Netlink 后端
    crates/netcore-cli        命令行客户端
    crates/netcore-gui        GTK4 桌面应用
    crates/netcore-xfce-plugin 由 plugins/xfce 引用
    plugins/gnome             GNOME Shell 扩展
    plugins/kde               KDE Plasma 6 小部件
    packaging/freebsd         rc.d、D-Bus、polkit、port 骨架
    docs/installation.md      两种安装模式与故障排查
    docs/ui.md                主题、字体、自绘图标与加速检测
    docs/authenticity.md      设备发现、SSID 原始字节与中文处理
    scripts/install-freebsd.sh
    scripts/install-freebsd-prebuilt.sh

## 测试

    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

已在 FreeBSD 15.1-RELEASE-p7 / Rust 1.98.1 / XFCE 4.20 环境完成：
GTK4 界面启动、D-Bus 调用、Wi-Fi 扫描、Netlink 打开与 Capsicum helper
进入能力模式、XFCE 插件在隔离 Xvfb 面板中加载并显示当前 SSID。GNOME
扩展与 KDE 插件通过各自的 mock D-Bus 集成测试；桌面选择、交互菜单与
--desktop all 完整 DESTDIR 安装均已验证；GUI 深/浅主题切换与自绘图标已实机截图。
36 个单元测试通过；另在 LC_ALL=C LANG=C 下验证 daemon/GUI 仍显示中文 SSID，
并通过 ssid_hex 保留原始字节。详细记录见 docs/testing.md。

## 许可

BSD-2-Clause，见 LICENSE。