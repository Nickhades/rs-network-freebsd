# Changelog

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
