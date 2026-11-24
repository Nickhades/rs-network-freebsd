# 测试记录

测试环境：FreeBSD 15.1-RELEASE-p7 amd64，Rust 1.98.1，XFCE 4.20.8，
GTK4 4.20.4，GNOME Shell 47.10，Plasma 6.7.5，Qt 6.11.2。本机 wlan0
已连接到 WPA2 网络，ue0 由 dhclient 配置。

## 自动测试

    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

结果：format 与 clippy 无警告；27 个单元测试全部通过（common 3、CLI 2、
daemon 19、GUI 3）。覆盖模型 JSON 往返、默认安全性枚举、接口分类与 MAC 格式化、
Netlink 消息解析与 socket bind、wpa_cli status/scan_results 解析、SSID
转义、DHCP 状态、CLI 目标选择。

## 实际运行（无 root）

守护进程：

    ./target/debug/netcore-daemon --bus session --wpa-backend ctrl --auth none

已验证：

- 成功连接会话 D-Bus，请求名称 org.netcore.NetworkManager。
- GetSnapshot 返回 wlan0（云涧，192.168.2.226）、ue0（10.83.82.58/59）、
  re0，MAC/MTU/IPv6 正确。
- 自动探测到 ctrl 后端（本机 wpa_supplicant 没有 D-Bus 服务）。
- NETLINK_ROUTE 监听启动；Capsicum 模式下 helper 进入能力模式（进程状态
  显示 SC），快照 netlink_active=true。
- ScanWifi 返回 9 个真实网络，SSID 中文解码正确，信号排序正确。
- CLI status/list/scan/doctor/monitor 正常；monitor 连续收到初始快照和
  扫描后的 SnapshotChanged。
- 非 root 修改操作返回 NotAuthorized，未触碰网络。
- GTK4 界面启动，显示设备、IP、网络列表；深色/浅色截图分别为
  docs/screenshot-gui.png 和 docs/screenshot-gui-light.png。
- 最终 release 构建的 netcore-daemon --capsicum 在会话总线启动，helper 进程
  状态为 SC，netlink_active=true。
- XFCE 插件在隔离 Xvfb + 私有 D-Bus 的临时面板中加载，显示 Wi-Fi 图标和
  当前 SSID “云涧”；日志确认 new item (type=object-type,
  name=netcore-plugin)。截图见 docs/screenshot-xfce-panel.png。
- 以 --desktop all 执行的完整 DESTDIR 安装通过，pkg-plist 中 28 个路径
  全部存在，GNOME JSON、KDE metadata、rc.d 语法与 XML 配置校验通过。

## 安装脚本验证

两种安装入口和桌面选择均在 FreeBSD 15.1 上以 DESTDIR 暂存方式验证：

- 桌面菜单：在伪终端中运行，显示 1 XFCE / 2 GNOME / 3 KDE / a / 0，
  输入 3 后只安装 KDE 目录，XFCE 与 GNOME 插件均未安装。
- 自动模式默认（当前 XFCE）：只构建并安装 XFCE 插件，不安装 GNOME/KDE。
- --desktop all 自动模式：构建 XFCE 与 KDE 插件，安装全部 28 个 pkg-plist
  路径。
- 预编译 --desktop gnome：在无编译调用的前提下只安装 GNOME 扩展，未安装
  XFCE/KDE 文件。
- 预编译 --desktop all：安装全部组件，28 个 pkg-plist 路径齐全。
- --desktop none：只安装核心组件，三个桌面插件目录都不存在。
- 预编译模式在 PATH=/usr/bin:/bin（无 cargo、无 qmake、无 cc）下成功，
  日志中没有编译调用，文件校验和与 target/release 产物一致。
- 自动回退：无编译环境时按所选桌面检查预编译产物并使用；GNOME-only
  回退只安装 GNOME 扩展。
- --desktop=gnome 等号写法和 1/2/3 序号写法均验证通过。
- 强制源码模式在无编译环境下正确失败，且没有留下部分安装。
- --check 输出所选桌面、编译环境、预编译产物状态和自动模式决策。
- 模式参数冲突会报错；预编译 wrapper 不能切入源码模式。
- --desktop all 的 DESTDIR 安装包含自绘应用图标，共 28 个 pkg-plist 路径。

## GUI 主题与自绘界面验证

- 默认 --theme dark 与 --theme light 分别启动成功，文本/背景/卡片/输入框/
  开关/标题栏颜色与桌面主题无关。
- 通过界面内“浅色/深色”按钮和合成鼠标点击完成主题切换；切换后所有自绘图标
  重绘，无重启。
- 字体检测读取当前系统字体 Noto Sans Mono CJK SC；全界面统一 700 字重，
  抗锯齿和轻 hinting 生效，底部状态栏显示字体族。
- 自绘 Wi-Fi、以太网、信号条、锁、刷新、加减、窗口控制、太阳/月亮、芯片、
  眼睛和旋转指示器图标；界面中不再调用系统图标名或 GtkPasswordEntry。
- 密码/隐藏网络对话框使用自绘关闭按钮、极简密码框和自绘眼睛切换；焦点环
  使用应用强调色，而非系统蓝色。
- 设置 GTK_THEME=Adwaita:light 启动默认深色，截图与常规深色一致，确认
  应用配色不依赖桌面主题。
- 自绘关闭按钮可关闭窗口；窗口控制不依赖系统标题栏按钮。
- 本机 /dev/dri 可用，底部显示 GPU 加速、活动 DRM 显卡 Phoenix1 和 16 核；
  GSK 渲染器与 CPU 型号在 tooltip 中。无 DRM 时的 CPU/Cairo 回退分支已编码。
- 自绘应用图标安装到 hicolor 主题目录，桌面文件 Icon 使用该图标。

## 桌面扩展验证

GNOME（由扩展实现代理使用私有 D-Bus mock 验证）：

- 所有 6 个 JS 文件通过 node --check。
- metadata.json 的 UUID、版本、shell-version、无 settings-schema 断言通过。
- 14 项工具函数、22 项 D-Bus 集成（含参数签名、信号、服务重启）、3 项
  清理、6 项错误映射测试通过。
- 未在实际 GNOME 会话中启用；UI 视觉效果未人工确认。

KDE Plasma：

- 使用 build.sh 构建 Netcore.Backend QML 插件成功。
- qmllint 零警告；qmlcachegen 4 个 QML 全部通过；qmlformat 解析通过。
- 私有 D-Bus mock 集成：snapshot 解析、网络排序、ConnectWifi、
  SetDeviceEnabled、busy/daemonAvailable 状态、密码窗口关闭均通过。
- CMake 规则已编写，但本机没有 cmake 可执行文件，未执行 CMake 构建。
- 未在实际 Plasma 会话中加载；视觉布局未人工确认。

XFCE：

- 调试版 .so 在 xfce4-panel 4.20.8 的独立 Xvfb 会话中加载成功。
- 插件直接通过 GDBus 获取当前快照，面板显示 Wi-Fi 符号和 SSID。
- 菜单交互未通过自动化点击验证。

## 未执行的危险测试

以下需要 root 且会改变测试机当前网络，未在开发机上执行：

- 实际 Wi-Fi 断开/连接与 DHCP 租约获取。
- ifconfig down/up。
- 系统总线 D-Bus 激活与 polkit 交互认证。
- rc.d 启动/停止。
- 真实 GNOME Shell / Plasma 会话安装。

相关代码路径已通过单元测试、编译检查和 mock 集成测试；发布前应在一次性
虚拟机或专用测试机执行完整集成测试。建议步骤：

1. 在测试接口（不要用管理连接的接口）上连接开放网络，验证 dhclient 启动
   和 IP 获取。
2. 断开，确认只杀死 netcore 启动的 dhclient，外部租约不受影响。
3. 使用错误密码，验证 35 秒超时、错误上报和网络清理。
4. 用 wheel 用户和普通用户分别调用 ConnectWifi，验证授权与 polkit。
5. 安装 D-Bus service 后使用 gdbus call 冷启动守护进程。
6. 在 XFCE/GNOME/KDE 真实会话安装插件，验证面板菜单与密码框。
7. 重启系统，验证 rc.d 启动、日志和 pidfile。

## 常用验证命令

    gdbus introspect --session --dest org.netcore.NetworkManager \
      --object-path /org/netcore/NetworkManager

    NETCORE_BUS=session netcore --json status

    gdbus monitor --session --dest org.netcore.NetworkManager

    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings