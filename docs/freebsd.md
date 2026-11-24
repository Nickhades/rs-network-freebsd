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
2. 控制套接字：自动查找包含控制 socket 的 /var/run/wpa_supplicant* 目录
   （也可用 --wpa-ctrl-dir 或 WPA_CTRL_DIR 指定）。FreeBSD ports 的
   wpa_supplicant 经常没有编译 D-Bus 支持，此时 netcore 调用 wpa_cli 并解析
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

dhclient(8) 由 netcore 用 -d（前台）启动，以便：

- 精确持有进程并停止；
- 捕获诊断输出；
- 避免 dhclient 自己 daemonize 后留下孤儿。

命令形如：

    dhclient -d -q <ifname>    # 实际路径由工具发现/--dhclient 决定

进程放入独立进程组；停止时先 SIGTERM 整个组，5 秒后 SIGKILL。若检测到
/var/run/dhclient/dhclient.<ifname>.pid 指向仍存活的外部 dhclient，则不启动
第二个客户端，而是沿用现有租约（这兼容 rc.d 的 dhclient 和手工配置）。netcore
只停止自己启动的进程，不会杀死外部 dhclient。

“是否有地址”以 getifaddrs 为准，而不是 dhclient 的退出码，因为 dhclient 可能
在前台持续续租。

## DHCP 后端选择

    --dhcp-backend auto      # 默认；优先 dhcpcd，否则 dhclient
    --dhcp-backend dhcpcd    # 固定 dhcpcd
    --dhcp-backend dhclient  # 固定 dhclient（旧系统回退）
    --no-dhcp                # 不启动 DHCP

auto 的探测顺序是：显式 `--dhcpcd`/环境变量 DHCPCD → PATH/常见目录中的
`dhcpcd` → 显式 `--dhclient`/DHCLIENT → `dhclient`。两条后端路径都会先校验
可执行位；不会通过 shell 或未校验的 PATH 拼接执行。

dhcpcd 后端使用 `-q -B --controlgroup wheel --metric 100`，优先读取
`/var/run/dhcpcd/<if>.sock`（并兼容 `/var/run/dhcpcd-<if>.sock`）的 JSON 事件；
控制 socket 不可用时回退到接口地址和 lease。外部 dhcpcd 不会被终止。
当前 DNS 仍由 dhcpcd/dhclient 自己管理；启动 dhcpcd 时未加 `-C resolv.conf`，
避免破坏现有 DNS。

## getifaddrs 与 if_data

- AF_LINK 的 ifa_data 指向 if_data，读取 MTU 与运行状态。
- sockaddr_dl 的 LLADDR(sdl) 等价于 sdl_data + sdl_nlen，代码据此生成 MAC。
- IPv4/IPv6 前缀由 netmask 的置位数计算。
- 只展示非 loopback、非已知虚拟接口（bridge、lagg、tun、tap 等），避免面板
  出现大量内核管道接口。

## D-Bus 与权限

系统总线服务文件让 dbus-daemon 以 root 激活：
/usr/local/share/dbus-1/system-services/org.netcore.NetworkManager.service。

D-Bus policy 允许所有本地用户向服务发送消息（只读功能需要），但只允许 root
拥有名称。真正的写权限在守护进程内检查：

1. root；
2. 调用者 UID 是否属于 --allow-group（默认 wheel）；
3. polkit action org.netcore.manage（允许交互认证）。

只读方法不触发授权。session 总线只用于开发，不应用于生产。

## Capsicum

--capsicum 让 netlink 解析器在子进程中：

1. 打开并绑定 NETLINK_ROUTE；
2. 调用 cap_enter(2)；
3. 仅通过已打开的管道/stdout 向父进程发送不可信事件的抽象标签；
4. 父进程仍负责所有特权网络操作。

这样，最容易被恶意内核/驱动消息影响的解析器处于能力模式，即使被攻破也无法
打开文件或网络。主守护进程不能整体进入能力模式，因为要 fork/exec DHCP 客户端、
打开 wpa control socket、读取配置。FreeBSD 提供 libcap_net/cap_net Casper
服务，可用于进一步把静态地址配置移入受限 helper；当前版本尚未使用。

## rc.d

netcore.rc 使用 daemon(8) 托管：

    /usr/sbin/daemon -p /var/run/netcore.pid -o /var/log/netcore.log \
        /usr/local/sbin/netcore-daemon --bus system ...

这样原守护进程保持前台（便于信号处理），daemon(8) 负责写 pidfile、重定向日志
和守护。status/stop 由 rc.subr 使用 pidfile 完成。

## 构建依赖

- Rust 1.95+（实际验证 1.98.1；xfce-panel 绑定要求 1.95）
- FreeBSD 15.1
- pkg-config
- GTK4 开发文件（GUI）
- libxfce4panel 2.0、GTK3 开发文件（XFCE 插件）
- dbus、wpa_supplicant、dhclient（基本系统）；dhcpcd（可选，pkg install dhcpcd）
- polkit（可选，推荐）
## 接口、工具与 SSID 发现

- 无线接口通过实际的 `ifconfig -g wlan` 组判断，不依赖 wlanN 名称；允许
  重命名接口。
- `--wpa-cli`、`--ifconfig`、`--dhclient`、`--dhcpcd` 未指定时依次检查
  环境变量、PATH 和常见 FreeBSD 目录，不要求工具在 /usr/local 或 /usr/sbin。
- 控制目录查找实际 socket，不假定只有一个固定目录。
- SSID 是 32 字节以内的任意八位组；`ssid` 是显示文本，`ssid_hex` 是精确
  字节。中文 UTF-8 在 C locale 下仍然还原为中文。
- 详细实现与验证见 docs/authenticity.md。