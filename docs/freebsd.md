# FreeBSD 技术细节与取舍

## 安装入口

netcore 提供自动与预编译两种一键安装方式：
- scripts/install-freebsd.sh：检测 Rust/GTK 编译环境；完整则自动编译，
  不完整时回退到 target/release 已有产物。
- scripts/install-freebsd-prebuilt.sh：只安装预编译产物，不调用任何编译器。

安装前可以选择桌面插件：XFCE、GNOME、KDE、全部或都不装。默认在终端中
显示序号菜单；非交互运行会选择当前桌面。除显式选择 all 外不会全量安装。
编译器与预编译产物检查均只针对所选项：只有选择 XFCE 才需要 GTK3，
只有选择 KDE 才需要 Qt6。一键卸载使用 scripts/uninstall-freebsd.sh；详细决策逻辑、DESTDIR/PREFIX
用法见 docs/installation.md。

## NETLINK_ROUTE

FreeBSD 13.2 引入 rtnetlink，15.1 的 AF_NETLINK 值为 38，NETLINK_ROUTE 为 0。
netcore 直接使用：

    socket(AF_NETLINK, SOCK_RAW, NETLINK_ROUTE)
    bind(... nl_groups = RTMGRP_LINK | RTMGRP_IPV4_IFADDR | RTMGRP_IPV6_IFADDR)

消息解析只信任长度字段，丢弃截断/未知消息。以 RTM_NEWLINK/DELLINK 和
RTM_NEWADDR/DELADDR 作为刷新触发器，不在热路径解析全部属性。

重要现实：FreeBSD 的 rtnetlink 覆盖仍在完善。部分地址事件可能不会产生通知
（内核头文件甚至仍把 RTM_NEWADDR 标为 not supported），因此 netcore 的
正确性不依赖 Netlink：守护进程始终保留 getifaddrs 轮询兜底，并在收到事件时
立即刷新。Netlink 只是降低延迟的加速路径。

以普通用户打开/绑定只读 Netlink 通知套接字在测试机上可行；实际修改接口仍需
root。

## wpa_supplicant 集成

netcore 支持两种后端，并在 auto 模式下探测：

1. D-Bus：服务 fi.w1.wpa_supplicant1，根对象 /fi/w1/wpa_supplicant1。
   通过 GetInterface(ifname) 获取动态 Interface 对象路径，再调用
   Scan/AddNetwork/SelectNetwork/Disconnect 并读取 State、BSSs、CurrentBSS。
   不使用固定接口对象路径。
自动探测顺序为 D-Bus 优先，失败后才尝试控制套接字。

2. 控制套接字：自动查找包含控制 socket 的 /var/run/wpa_supplicant* 目录
   （也可用 --wpa-ctrl-dir 或 WPA_CTRL_DIR 指定）。FreeBSD ports 的
   ports 的 wpa_supplicant-devel 带 D-Bus；基本系统版本可能未编译，此时 netcore 调用 wpa_cli 并解析
   status/scan_results。

连接安全性映射：

- 开放：key_mgmt=NONE
- WPA/WPA2：key_mgmt=WPA-PSK，psk=<password>
- WPA3-SAE：key_mgmt=SAE，sae_password=<password>
- WEP、企业级 802.1X：明确拒绝（避免不安全或半成品实现）

注意：真实的 wpa_supplicant D-Bus API 没有通用的 Interface.Connect；连接是
AddNetwork + SelectNetwork 后等待 State=completed。netcore 按实际 API 实现。

扫描结果的 SSID 在 ctrl 后端是 wpa_cli 的转义形式（\xNN）。解析器按字节
解码，不先做 UTF-8 lossy 转换，也不 trim SSID；合法 UTF-8（包括中文）原样
显示，非 UTF-8 字节以 \xNN 显示并保留 ssid_hex。按原始字节去重，保留最强
BSS。

## DHCP

netcore 只使用 dhcpcd(8)。命令形如：

    dhcpcd -q -b -C resolv.conf --controlgroup wheel --metric 100 <ifname>

- `-b` 让 dhcpcd 后台化；daemon 通过控制 socket 和 lease 追踪，不持有前台
  子进程。
- `-C resolv.conf` 禁止 dhcpcd 修改 DNS；daemon 在 BOUND 后把活动接口的
  域名服务器合并写入 `/etc/resolv.conf`，断开时移除该接口条目。
- `--metric 100` 让 dhcpcd 管理路由优先级；停止时 dhcpcd 负责清除自己的
  路由。
- 若检测到系统级或按接口的外部 dhcpcd，则沿用其租约，不启动竞争实例，
  也不改写 DNS。

控制 socket 优先读取 `/var/run/dhcpcd/<if>.sock`（兼容
`/var/run/dhcpcd-<if>.sock`），发送 `{"command":"listen"}`。socket
不可用或事件不是 JSON 时，回退到接口地址和 `/var/db/dhcpcd/*.lease`。
停止时先发送 `{"command":"terminate"}`，失败则执行 `dhcpcd -k <ifname>`。

启动参数只有：

    --dhcpcd <path>       # 显式路径；默认自动发现
    --resolv-conf <path>  # 默认 /etc/resolv.conf，供测试/自定义镜像
    --no-dhcp             # 静态地址

推荐 rc.conf：

    wlans_rtw890="wlan0"
    create_args_wlan0="country CN"
    ifconfig_wlan0="up"
    dhcpcd_enable="NO"
    wpa_supplicant_program="/usr/local/sbin/wpa_supplicant"

不要设置 `ifconfig_wlan0="WPA"`；否则 rc 脚本会抢先启动 wpa_supplicant，
daemon 只能连接它而无法管理其生命周期。netcore 当前假设 wpa_supplicant 已
运行，D-Bus 优先，控制套接字回退。
