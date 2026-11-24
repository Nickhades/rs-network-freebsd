# 架构

## 分层

    +--------------------------------------------------------------+
    | 前端：CLI / GTK4 / XFCE / GNOME Shell / Plasma               |
    +------------------------------- D-Bus ------------------------+
    | org.netcore.NetworkManager                                   |
    |   GetSnapshot / ListDevices / ScanWifi / ConnectWifi ...       |
    |   SnapshotChanged / DeviceChanged / NetworkScanFinished        |
    +--------------------------------------------------------------+
    | netcore-daemon                                                |
    |   Manager（状态机、操作串行化、快照去重）                       |
    |   WifiService（D-Bus 或 ctrl socket）                          |
    |   LinkState 状态机（Down/Scanning/Authenticating/...）         |
    |   DhcpClient 后端（dhclient / dhcpcd，统一事件流）              |
    |   NetlinkMonitor（事件加速；轮询兜底）                         |
    |   Authorizer（root / 组 / polkit）                             |
    +--------------------------------------------------------------+
    | FreeBSD：getifaddrs(3) / NETLINK_ROUTE / ifconfig / DHCP     |
    |          wpa_supplicant D-Bus / unix control socket           |
    +--------------------------------------------------------------+

## 状态来源

netcore 不把 wpa_supplicant 的内部状态当作最终事实。设备状态由三层合成：

1. 内核事实：getifaddrs 返回的 IFF_UP、IFF_RUNNING、IPv4/IPv6 地址、MAC、
   MTU。
2. 关联事实：wpa_supplicant 的 State/BSSs/CurrentBSS（D-Bus）或
   wpa_state/ssid/bssid（ctrl）。
3. 操作意图：Manager 当前正在执行的连接步骤（Connecting、IpConfig、
   Disconnecting）以及最近一次失败原因。

因此即使 NETLINK_ROUTE 漏发事件，周期刷新仍会把 GUI/CLI 纠正到真实状态。
每次刷新生成一个 ManagerSnapshot；如果内容与上次语义相同（忽略
updated_at），不会重复广播信号。

## 连接流程

    GUI/CLI
      |
      | ConnectWifi(ifname, ssid, password, hidden)
      v
    Authorizer（root / wheel / polkit）
      |
      v
    Manager.connect_wifi
      |-- 校验接口、SSID、安全性、密码
      |-- 设置操作状态 Connecting，刷新快照
      |-- ifconfig <if> up
      |-- wpa_supplicant AddNetwork + SelectNetwork / wpa_cli
      |-- 等待 wpa_state=completed
      |-- 设置 IpConfig，刷新快照
      |-- DhcpManager.ensure：启动 dhclient/dhcpcd 并等待事件流 BOUND
      |-- 设置 LinkState::Connected，清除操作状态，广播快照
      v
    SnapshotChanged -> 所有前端

失败路径：停止 DHCP、断开 Wi-Fi 关联、记录 last_error、回到刷新循环，并把
D-Bus 方法错误返回给调用者。D-Bus 后台错误还会通过 Error 信号广播。

## 链路状态机

Manager 内的 LinkState 把用户请求和两类底层事件串起来：

    Down -> Scanning -> Authenticating -> Associated
         -> ConfiguringIp -> Connected
    Connected --roam/Disconnect--> ... -> Down

- `Associated` 只表示 CTRL-EVENT-CONNECTED，绝不等于可上网；
- `Connected` 只在 DHCP 事件 `Bound` 之后设置；
- 连接失败、租约丢失或接口消失会回滚到 `Down`；
- 断开必须经过 `Disconnecting`，避免状态跳变。

dhclient 没有事件 socket，因此它的后端在内部观察接口地址和 lease 文件；
dhcpcd 优先使用控制 socket 的 JSON 事件，不可用时使用同样的 lease 兜底。
这两类观察被封装在 `DhcpClient` 后端内，Manager 只消费 DhcpEvent。

## 并发模型

- tokio 多线程运行时。
- Manager 内部使用 Arc 共享，所有方法通过 op_lock 串行化变更；读操作可并发。
- refresh_lock 防止多个刷新交错导致旧快照覆盖新快照。
- 操作状态、错误与活动设备使用 async RwLock；网络扫描缓存为
  HashMap<ifname, Vec<WifiNetwork>>。
- 每个 Wi-Fi 状态读取有 3 秒超时，避免慢 wpa_supplicant 阻塞 GUI 刷新。
- D-Bus 服务对象每个方法只持有短暂锁，不在持锁期间执行外部命令。

## D-Bus 契约

JSON 文档作为字符串返回，是刻意取舍：

- Rust 端仍然序列化强类型模型；
- GJS、QML、Python、shell 不需要维护 a{sv}；
- 方法参数保持基本类型，Introspection 仍可读；
- 协议演进只需保证 JSON 向后兼容。

完整方法/信号见 dbus-api.md。

## 模块职责

- crates/netcore-common/src/model.rs：所有共享模型、枚举与 JSON 协议。
- crates/netcore-daemon/src/manager.rs：状态合成、操作编排、快照去重。
- crates/netcore-daemon/src/wifi/：wpa_supplicant 双后端及扫描解析。
- crates/netcore-daemon/src/dhcp/：DhcpClient trait、dhclient/dhcpcd 后端、
  lease 解析与 BOUND 事件编排。
- crates/netcore-daemon/src/link.rs：统一链路状态机。
- crates/netcore-daemon/src/netlink.rs：FreeBSD rtnetlink 监听/Capsicum helper。
- crates/netcore-daemon/src/auth.rs：D-Bus 调用者 UID、组与 polkit 检查。
- crates/netcore-daemon/src/dbus.rs：接口、错误映射、信号泵。
- crates/netcore-client：异步客户端、代理、信号流。
- crates/netcore-gui：GTK4；UI 主线程只处理控件，D-Bus 在后台线程。
- plugins/xfce：xfce4-panel 内的常驻 GObject 插件。
- plugins/gnome、plugins/kde：桌面原生面板扩展。

## 错误映射

ManagerError.code 对应 D-Bus 错误名：

    invalid_argument      -> org.netcore.NetworkManager.Error.InvalidArgument
    not_authorized        -> ...NotAuthorized
    not_found             -> ...NotFound
    backend_unavailable   -> ...BackendUnavailable
    busy                  -> ...Busy
    operation_failed      -> ...OperationFailed
    unsupported           -> ...Unsupported

前端应同时显示 message，并按 code 决定是否提示密码/权限。

## 为什么不是 NetworkManager 的 D-Bus API

org.freedesktop.NetworkManager 已被其他实现占用，且其完整接口面很大。netcore
使用独立名称与稳定 JSON 协议，避免与现有服务冲突，也便于逐层扩展。