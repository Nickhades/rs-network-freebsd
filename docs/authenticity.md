# 设备发现与 SSID 真实性

netcore 不假定运行它的机器一定叫 wlan0、一定把工具装在 /usr/sbin，也不假定
SSID 一定是 UTF-8。所有名称都来自内核、wpa_supplicant 或实际探测结果。

## SSID 是字节序列

802.11 SSID 最长 32 个八位组，可以是中文 UTF-8、带首尾空格、带控制字符，
也可以是非 UTF-8 字节。模型同时暴露：

- ssid：给 UI/日志的显示文本。有效 UTF-8（包括中文）原样保留；非 UTF-8
  字节显示为逐字节的 \xNN，不使用 U+FFFD 替换。
- ssid_hex：完整 SSID 的小写十六进制，始终是无损事实来源。

控制套接字后端以 LC_ALL=C/LANG=C 调用 wpa_cli，按原始字节读取
status/scan_results，不先做 String::from_utf8_lossy：

1. 保留行尾以外的所有 SSID 字节，包括空格、制表符和 NUL。
2. 解析 wpa_supplicant 的 \xNN、\n、\r、\t、\\、\" 转义。
3. 不 trim SSID，不按显示文本去重，不把无效 UTF-8 替换成 �。
4. 按 ssid_hex 去重和匹配已连接网络。
5. D-Bus 后端直接读取 SSID/BSSID 的 Vec<u8>，走同一套解码。

因此 FreeBSD 默认 C locale 下 wpa_cli 输出 \xe4\xba... 时，netcore 会还原
为 UTF-8 中文，并在 JSON/GUI/CLI 中显示实际名称；同时保留原始字节。

非 UTF-8 SSID 可以在扫描结果和 JSON 中完整查看，但不能通过字符串形式的
ConnectWifi 接口安全重连；主 GUI 会阻止这种连接，而不是把转义文本当成
另一个 SSID 发出去。

## 无线接口不靠名字前缀

FreeBSD 的无线接口通常叫 wlanN，但可以重命名。netcore 在每次刷新时执行
实际解析出的 ifconfig -g wlan，用接口组判断是否无线；DeviceKind::Wifi
不再依赖 starts_with("wlan")。GUI、CLI 和 panel 插件都使用快照里的真实
ifname，不写死 wlan0。

## 工具与控制目录自动发现

--wpa-cli/--dhclient/--ifconfig 未指定时，按以下顺序查找：

1. 对应环境变量（WPA_CLI/DHCLIENT/IFCONFIG）。
2. PATH。
3. FreeBSD 常见目录（/sbin、/usr/sbin、/usr/local/sbin 等）。

wpa_supplicant 控制目录也是可选的：显式 --wpa-ctrl-dir 或 WPA_CTRL_DIR
优先，否则查找包含控制 socket 的 /var/run/wpa_supplicant*；找不到时让
wpa_cli 使用自身默认值。wpa_cli 调用会带上发现的 -p 路径。

CLI doctor 和 GUI 加速检测也使用相同的解析器，不再写死 /usr/sbin/wpa_cli、
/sbin/sysctl 或 wlan0。CLI 的 scan/connect/disconnect 省略 ifname 时会从
快照选择第一个合适的接口。

## 字体、GPU 与主题

系统字体由 GTK 当前字体解析，GPU 由 /dev/dri + DRM PCI ID + pciconf 实际
探测；没有 DRM 时回退 Cairo。深/浅两套颜色是刻意的应用设计常量，不读取
桌面主题，因此不会随不同 FreeBSD 桌面的系统颜色变化。

## 兼容性

ssid_hex 是协议 v1 的可选新增字段；旧消费者忽略它，仍可读取 ssid。新消费者
遇到旧守护进程没有 ssid_hex 时按显示文本兼容。

## 验证

- LC_ALL=C LANG=C 启动 session daemon，scan --json 返回：
  云涧 / e4ba91e6b6a7，包厢 / e58c85e58ea2。
- 同一 C locale 下启动 GUI，中文 SSID 正常显示。
- 单元测试覆盖 UTF-8 中文、非 UTF-8 字节、首尾空格、制表符、\xNN 解码、
  hex 去重和转义往返。
- 无线接口分类单元测试使用 radio0 + wireless=true，证明不依赖 wlan 前缀。
- CLI doctor 输出实际解析到的 ifconfig/dhclient/wpa_cli 路径和真实接口。
