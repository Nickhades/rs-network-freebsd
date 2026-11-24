# Changelog

## v0.3.3

- rc.d post-start 在 daemon(8) 未留下 pidfile 时，用 pgrep 查找
  netcore-daemon 并补写 /var/run/netcore.pid，修复 service status 永远
  显示未运行的问题。

## v0.3.2

- 修复 rc.d 脚本 start_postcmd 指向不存在的 netcore_postcmd，导致
  `service netcore start` 报 eval 错误、没有 pidfile、status 显示未运行。

## v0.3.1

- 修复 GUI 在动作失败后重新渲染快照时的 RefCell 双重借用 panic。
- 修正 rc.conf 文档：当前版本让系统 rc 启动 wpa_supplicant 和 dhcpcd，
  netcore 连接并沿用；避免重启后没有 wpa 控制 socket 导致扫描失败。
- 面板配置修复属于本机用户配置，不进入发布产物。

## v0.3.0

- wpa_supplicant 自动探测改为直接优先连接 D-Bus（fi.w1.wpa_supplicant1），
  失败后才回退到控制套接字；不再依赖 owner 预检查。
- D-Bus 扫描等待 PropertiesChanged 的 Scanning，连接等待 State=completed；
  ctrl 后端继续使用 ATTACH 事件。
- 移除 dhclient：daemon 只使用 dhcpcd，删除 --dhcp-backend/--dhclient 和
  DhclientBackend；--dhcpcd 可显式指定路径。
- dhcpcd 使用 -q -b -C resolv.conf --controlgroup wheel --metric 100，
  控制 socket/lease 事件驱动。
- 新增 daemon 级 resolv.conf 写入：BOUND 合并活动接口 DNS，STOPPED/EXPIRED
  移除对应条目；默认路径 /etc/resolv.conf，可用 --resolv-conf 覆盖。
- 安装脚本检查 ports 的 wpa_supplicant D-Bus 支持和 dhcpcd；README 增加
  推荐 rc.conf，避免 ifconfig_wlan0="WPA" 抢占 wpa_supplicant。
- 路由仍由 dhcpcd 按 metric 管理；daemon 当前不直接调用 route(8)。

## v0.2.0

- 新增 `DhcpClient` trait、`DhcpEvent` 事件流，以及 dhclient/dhcpcd 双后端。
- 新增 `--dhcp-backend auto|dhclient|dhcpcd` 与 `--dhcpcd`，auto 优先 dhcpcd。
- dhcpcd 后端支持控制 socket 的 listen JSON 事件，并在不可用时回退到接口
  地址/lease 观察；外部 dhclient/dhcpcd 不会被杀死。
- 新增统一 `LinkState` 状态机，区分 Associated、ConfiguringIp 与 Connected。
- wpa_supplicant ctrl 后端加入 ATTACH 事件读取，扫描等待
  CTRL-EVENT-SCAN-RESULTS，连接等待 CTRL-EVENT-CONNECTED；移除固定 2.2 秒
  扫描 sleep。
- wpa_supplicant D-Bus 后端等待 PropertiesChanged Scanning/State，而不是
  固定间隔轮询。
- 新增 lease 解析、控制事件 JSON、fake 进程生命周期、proptest SSID 往返
  测试；新增 FreeBSD Cirrus CI 配置。
- 安装脚本检测 dhcpcd/dhclient；端口骨架提供可选 DHCPCD 依赖。
- 已知限制：路由/DNS 仍由 DHCP 客户端管理；dhcpcd 控制 socket 的 JSON
  布局存在版本差异，后端保留 lease 回退。