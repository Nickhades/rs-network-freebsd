# 安全模型

## 信任边界

- 守护进程 netcore-daemon 是唯一以 root 执行网络变更的组件。
- CLI、GUI、面板插件均以桌面用户运行，通过系统 D-Bus 调用守护进程。
- D-Bus 消息内容不可信：接口名、SSID、密码、路径长度与特殊字符都要校验。
- wpa_supplicant 与内核返回的数据也不可信；解析器必须防越界与长度错误。

## 授权

修改方法（ConnectWifi、DisconnectDevice、SetDeviceEnabled、SetWifiEnabled）
在 DaemonService 层检查调用者，而不是只依赖 D-Bus policy：

1. 解析发送者的 unique name，通过 org.freedesktop.DBus 查询 UID。
2. UID 0 直接允许。
3. --allow-group（默认 wheel）成员允许。
4. 否则调用 polkit CheckAuthorization(org.netcore.manage,
   AllowUserInteraction)。
5. 全部失败返回 org.netcore.NetworkManager.Error.NotAuthorized。

只读方法（GetSnapshot、ListDevices、GetVersion、ScanWifi）允许本地用户。
ScanWifi 会触发射频扫描，但没有管理权限不泄露密码。

session 总线模式为了开发方便信任调用者；生产必须使用系统总线和默认授权。

## 密码处理

- 密码不会写入磁盘、不会写入 tracing 日志。
- D-Bus 后端将密码作为 D-Bus 方法体传给 wpa_supplicant，不出现在命令行。
- ctrl 后端通过 wpa_cli set_network 传密码。密码会短暂出现在 wpa_cli 的
  argv 中，同机 root 用户可见；普通用户通常不可见。这个向量来自 wpa_cli
  的命令行接口，后续版本可改为直接实现 wpa_ctrl 协议消除。
- 密码在内存中随请求结束释放；Rust String 不保证清零。强烈建议后续使用
  zeroize。
- 临时网络不会 save_config，不会改写 /etc/wpa_supplicant.conf。

## 命令执行

所有外部命令都通过 tokio::process::Command 以参数数组执行，不经过 shell；
接口名先通过白名单校验（字母数字、下划线、点、连字符）并限制 IFNAMSIZ。
SSID 会校验长度不超过 32 字节；扫描结果按原始字节保存（ssid_hex），
控制套接字字符串按 wpa_supplicant 规则转义，不对中文或非 UTF-8 字节做
有损替换。

## Capsicum

--capsicum 将 NETLINK_ROUTE 套接字与消息解析移到子进程，子进程在完成 socket
和 bind 后调用 cap_enter(2)。父进程只消费固定的事件标签。限制：

- 主守护进程无法整体进入能力模式，因为需要 fork/exec dhcpcd 并访问
  wpa_supplicant/dhcpcd 控制套接字路径。
- Netlink helper 仍与守护进程同 UID；它是解析器隔离，不是完整沙箱。
- 后续可用 FreeBSD Casper 的 cap_net/cap_dns 服务拆分特权操作，并把 wpa
  控制套接字预打开后限制 fd 权限。

## D-Bus 攻击面

- 任意本地用户都能调用只读接口，可能造成频繁 GetSnapshot/ScanWifi 的 CPU
  消耗；生产可在 D-Bus policy 增加速率/组限制。
- 方法操作通过 op_lock 串行化，避免并发连接/断开造成 DHCP 竞态。
- zbus 负责消息大小与序列化安全检查；我们只接受有界字符串。
- 返回的 JSON 中包含 SSID 与 BSSID，不含密码。

## 进程与租约

- dhcpcd 以 -b 后台运行；停止时优先发送控制 socket terminate，失败后执行
  dhcpcd -k，并清理自己写入的 DNS 条目。
- 只停止自己启动的 dhcpcd；对外部（系统全局或按接口）dhcpcd 只读取租约，
  不发送终止信号，也不改写 DNS。
- 守护进程收到 SIGTERM/SIGINT 时停止自己的 dhcpcd 并退出。
- 若守护进程被 SIGKILL，dhcpcd 可能继续续租；重新启动后会通过 pidfile 或
  控制 socket 检测并沿用，不会重复启动。
- dhcpcd 控制 socket 由 root/wheel 权限限制；后端只接受固定路径，并在连接
  和读取上设置超时。`--controlgroup wheel` 让控制组访问面收窄到 wheel；
  若目标系统不支持该选项，应审计并改用专门的 netcore 组。
- dhcpcd 路径经过 `is_executable` 校验，不拼接 shell；`--dhcpcd` 只能指向
  实际可执行文件，防止 PATH 注入。
- daemon 用活动接口的 DNS 列表原子式重写 `/etc/resolv.conf`，只删除自己
  记录的接口条目；不会把密码或任意客户端输入写入该文件。路由由 dhcpcd
  管理，daemon 不直接调用 route(8)。

## 建议的加固路线

1. 用直接 wpa_ctrl 协议替换 wpa_cli，消除密码 argv 暴露。
2. 用 Capsicum 预打开 dhcpcd 和 wpa/dhcpcd 控制 socket，限制 fd 权限。
3. 接入 FreeBSD Casper cap_net 完成地址/路由操作，并把 DNS 所有权移入
   daemon。
4. polkit 集成测试与 action 文件签名/打包。
5. 为 D-Bus 方法增加每个 UID 的速率限制。
6. 使用 zeroize/SecretBox 保存密码并清理内存。