# netcore GNOME Shell 扩展

netcore 是 FreeBSD 上的原生网络管理器。本扩展在 GNOME Shell 顶部面板中提供一个
网络指示器，直接通过系统 D-Bus 与 `netcore-daemon` 通信，不依赖
`netcore-cli`，也不依赖任何第三方 GNOME 扩展。

- 扩展 UUID：`netcore@freebsd.local`
- 目标环境：GNOME Shell 45 / 46 / 47 / 48（开发机为 47.10）
- 显示语言：简体中文
- 许可证：BSD-2-Clause（与 netcore 项目一致）

## 功能

- 面板指示器使用 Adwaita 符号图标，随连接状态变化：已连接、正在连接/断开、
  离线、守护进程不可用。
- 鼠标悬停时显示状态提示（tooltip），例如当前 SSID、IP 地址或错误原因。
- 弹出菜单包含：
  - 总体连接状态与当前设备/IP，或守护进程错误信息；
  - Wi-Fi 总开关；
  - “扫描 Wi-Fi 网络”动作；
  - 按信号强度从强到弱排序的 Wi-Fi 列表，显示安全类型与信号质量百分比；
  - 当前连接的 SSID 带勾选标记；
  - 开放网络点击后直接连接；需要密码的网络弹出 GNOME Shell 原生模态密码框；
  - 非回环设备的“断开/连接”动作；
  - “打开网络管理器”，通过 `Gio.Subprocess` 启动 `netcore-gui`。
- 守护进程未运行、D-Bus 调用失败、权限不足、超时等情况都会在菜单状态行中
  给出提示，不会向 GNOME Shell 抛出未捕获异常。
- 支持 D-Bus 信号 `SnapshotChanged`，并每 5 秒轮询 `GetSnapshot()` 作为兜底；
  同时监听系统总线上的服务名变化，以便守护进程重启后自动恢复。

## 文件说明

| 文件 | 作用 |
|---|---|
| `metadata.json` | 扩展元数据（UUID、名称、shell-version 45–48、version 1）。 |
| `extension.js` | ESM 入口，`enable()` 创建指示器，`disable()` 销毁。 |
| `indicator.js` | PanelMenu.Button、菜单内容、状态渲染、动作、密码框与清理逻辑。 |
| `menuItems.js` | 动态 Wi-Fi/设备菜单行和“仅在内容变化时重建”的签名计算。 |
| `panelTooltip.js` | 面板图标的悬停提示。GNOME Shell 的 PanelMenu.Button 没有原生 tooltip API，因此使用 Shell 自己截图 UI 同款的 `St.Label` + `Main.uiGroup` 方案。 |
| `netcoreClient.js` | 直接基于 `Gio.DBusConnection` 的异步 D-Bus 客户端。 |
| `utils.js` | 中文标签、信号质量换算、安全类型判断等纯函数。 |
| `stylesheet.css` | 菜单行、状态文字、密码框布局样式。 |
| `README.md` | 本文档。 |

## 环境要求

- GNOME Shell 45–48（ESM 扩展格式；GNOME 44 及更早版本不支持）。
- GJS（GNOME Shell 自带）。
- 正在系统总线上运行、并实现 `org.netcore.NetworkManager` 协议 v1 的
  `netcore-daemon`。
- 如需“打开网络管理器”按钮生效，`netcore-gui` 必须在 GNOME Shell 进程的
  `PATH` 中。
- 修改网络状态的操作由守护进程通过 polkit（`org.netcore.manage`）授权。

## 安装

扩展本身没有编译步骤，源文件可以直接安装。**请不要把本仓库路径直接软链接到
扩展目录**；请复制到 GNOME Shell 的标准扩展目录，因为 Shell/GJS 对非 ASCII
路径的文件 URL 处理并不总是可靠。

### 方式一：复制到用户扩展目录（推荐）

FreeBSD 和 Linux 的用户目录都是：

```
~/.local/share/gnome-shell/extensions/
```

从 netcore 仓库根目录执行：

```sh
install -d ~/.local/share/gnome-shell/extensions
rm -rf ~/.local/share/gnome-shell/extensions/netcore@freebsd.local
cp -R plugins/gnome/netcore@freebsd.local \
      ~/.local/share/gnome-shell/extensions/
```

### 方式二：使用 gnome-extensions CLI

先打包，再安装。注意 zip **根目录必须直接包含 `metadata.json`**，所以要在
扩展目录内部打包，不要把整个目录作为 zip 的顶层条目：

```sh
cd plugins/gnome/netcore@freebsd.local
zip -r /tmp/netcore@freebsd.local.zip .
gnome-extensions install --force /tmp/netcore@freebsd.local.zip
```

如果 FreeBSD 上没有 `gnome-extensions` 命令（部分安装只提供图形界面的
“扩展”应用），请使用方式一，然后通过 “扩展” 应用启用。

### 方式三：安装到系统目录

FreeBSD 的系统扩展目录是：

```
/usr/local/share/gnome-shell/extensions/
```

Linux 通常为 `/usr/share/gnome-shell/extensions/`。

```sh
sudo cp -R plugins/gnome/netcore@freebsd.local \
          /usr/local/share/gnome-shell/extensions/
```

系统目录中的扩展对所有用户可见，但升级时需要 root 权限替换文件。

## 启用与重载

```sh
gnome-extensions enable netcore@freebsd.local
gnome-extensions info netcore@freebsd.local
```

也可以在 “扩展”（Extensions）应用中打开。

安装或更新后通常需要让 GNOME Shell 重新加载扩展：

- **X11**：按 `Alt+F2`，输入 `r` 回车，重启 Shell。
- **Wayland**：GNOME Shell 不能原地重启，必须注销后重新登录；新安装的扩展
  在注销/登录前不会出现在加载列表中。
- 如果只是重新启用扩展，`gnome-extensions disable` 再 `enable` 通常即可；
  更新 JS 文件后仍建议重启会话，避免旧模块缓存。

> 按任务要求，本次交付**没有**在正在运行的桌面会话中安装或启用扩展。

## 测试

### 1. 确认守护进程在系统总线上

```sh
gdbus call --system \
  --dest org.netcore.NetworkManager \
  --object-path /org/netcore/NetworkManager \
  --method org.netcore.NetworkManager.GetSnapshot
```

成功时会返回一个包含 `protocol_version`、`state`、`devices` 等字段的 JSON
字符串。若返回
`org.freedesktop.DBus.Error.ServiceUnknown`，说明守护进程没有在系统总线
运行；面板会显示 “netcore 守护进程不可用”。

### 2. 面板状态

逐项检查：

- 守护进程运行时，图标随 `state` 变化：
  - `connected`：有线图标、Wi-Fi 信号图标或通用网络图标；
  - `connecting` / `disconnecting`：`network-wireless-acquiring-symbolic`；
  - `offline`：`network-offline-symbolic`，Wi-Fi 关闭时为
    `network-wireless-disabled-symbolic`；
  - 守护进程不可用：`network-error-symbolic`。
- 鼠标悬停 350 ms 后出现 tooltip；打开菜单时 tooltip 消失。

### 3. 菜单动作

- 打开菜单，确认顶部显示“已连接 · 本地网络”或“离线”等总体状态。
- 切换 “Wi-Fi” 开关，确认守护进程状态和网络列表随之更新。
- 点击 “扫描 Wi-Fi 网络”，确认出现“正在扫描…”提示，随后列表刷新。
- 点击开放网络，应直接发起连接；点击 WPA/WPA2/WPA3/WEP 网络，应弹出
  原生密码框。密码框右侧的眼睛按钮可以临时显示密码。
- 输入错误密码并连接，菜单因动作失败而显示错误提示，守护进程不会被崩溃。
- 当前 SSID 所在行应有勾选图标，并显示安全类型与信号质量。
- 对非回环设备（如 `wlan0`、`em0`）点击“断开”/“连接”，观察状态变化。
- 点击 “打开网络管理器”；若 `netcore-gui` 不存在，状态行应给出可读错误而
  不是抛出异常。
- 用 `service netcore stop`（或等价方式）停止守护进程，面板应显示错误状态；
  重新启动后最多 5 秒内应自动恢复。
- 执行 `gnome-extensions disable netcore@freebsd.local` 再启用，确认没有残留
  的 tooltip、菜单或定时器；Shell 日志中不应出现扩展异常。

### 4. 查看日志与调试

- `Alt+F2`，输入 `lg` 打开 Looking Glass，在 “Extensions” 页查看 JS 异常。
- Linux 上可用 `journalctl -f -o cat /usr/bin/gnome-shell`；FreeBSD 上查看
  GNOME 会话日志或 `/var/log/messages` 中与 gnome-shell 相关的记录。
- 扩展在 D-Bus 回调异常时使用 `console.error` 输出带 `[netcore]` 前缀的
  信息，便于过滤。

## 设计说明

### 直接、异步的 D-Bus

- 使用 `Gio.DBusConnection`（`Gio.bus_get` 异步获取连接）直接调用
  `org.netcore.NetworkManager`。
- 所有方法都走 `connection.call()` 的异步回调，**没有** `call_sync`、
  `Gio.DBusProxy.new_sync` 等阻塞调用，避免卡住 GNOME Shell 主循环。
- 全部请求共享一个 `Gio.Cancellable`；`disable()` 时取消。
- 方法签名严格对应协议：
  - `GetSnapshot() -> s`
  - `ScanWifi(s) -> s`
  - `ConnectWifi(sssb) -> s`（`hidden` 固定为 `false`）
  - `DisconnectDevice(s) -> s`
  - `SetDeviceEnabled(sb) -> s`
  - `SetWifiEnabled(b) -> ()`
- 信号：`SnapshotChanged(s)`。另外订阅
  `org.freedesktop.DBus.NameOwnerChanged` 过滤
  `org.netcore.NetworkManager`，用于立即感知守护进程启动/退出。
- 每 5 秒调用一次 `GetSnapshot()` 作为信号丢失时的兜底，并对并发的
  `GetSnapshot` 做去重排队。
- 所有 JSON 解析都做容错；未知字段被忽略（协议 v1 允许增量字段）。仅当
  `devices` 是数组时才接受快照。

### 状态与图标

| ManagerSnapshot.state | 面板图标 |
|---|---|
| `connected`（Wi-Fi） | 按已连接网络的信号选择 `network-wireless-signal-*-symbolic` |
| `connected`（以太网） | `network-wired-symbolic` |
| `connected`（其他/未知） | `network-transmit-receive-symbolic` |
| `connecting` / `disconnecting` | `network-wireless-acquiring-symbolic` |
| `offline`（Wi-Fi 可用但关闭） | `network-wireless-disabled-symbolic` |
| `offline`（其他） | `network-offline-symbolic` |
| 守护进程不可用 | `network-error-symbolic` |

状态行和 tooltip 使用同一套状态判断；tooltip 额外包含当前 SSID 与 IPv4 地址。

### Wi-Fi 列表与密码框

- 网络来自快照中 Wi-Fi 设备的 `networks` 字段，按 `signal` 从强到弱排序；
  信号质量百分比严格复刻
  `netcore_common::model::WifiNetwork::quality_percent()`。
- 安全类型按模型映射为“开放 / WEP / WPA / WPA2 / WPA3 / 企业级 / 未知”。
  是否弹密码框遵循 `Security::requires_password()`：仅 `open` 和 `unknown`
  直接连接，其余类型弹出密码框。
- 密码框使用 `St.PasswordEntry`。它是 `St.Entry` 的子类，通过
  `show-peek-icon` 提供 GNOME 原生的“显示密码”眼睛按钮；入口获得键盘焦点，
  回车触发“连接”，Esc/“取消”关闭。
- 当前连接判断同时参考网络的 `connected` 字段与设备 `ssid`，以兼容守护进程
  只更新设备 SSID 的情况。
- 多 Wi-Fi 设备时，优先使用 `active_device` 对应的 Wi-Fi 设备，否则使用第一个
  有 SSID 或扫描结果的 Wi-Fi 设备。

### 设备动作

- 过滤 `kind === "loopback"` 的设备。
- 已连接/正在连接/正在获取地址：显示“断开”，调用
  `DisconnectDevice(ifname)`。
- 未连接/失败/不可用/未知：显示“连接”，调用
  `SetDeviceEnabled(ifname, true)`（Wi-Fi 设备此时只是启用接口，之后仍需在
  Wi-Fi 列表中选择网络）。
- 未托管（`managed == false`）或正在断开的设备不显示动作。

### 打开 netcore-gui

```js
const launcher = new Gio.SubprocessLauncher({
    flags: Gio.SubprocessFlags.STDOUT_SILENCE |
        Gio.SubprocessFlags.STDERR_SILENCE,
});
const process = launcher.spawnv(['netcore-gui']);
process.wait_async(null, (source, res) => {
    try {
        source.wait_finish(res);
    } catch {
        // 启动失败已经由 spawnv() 的同步异常报告
    }
});
```

- 通过 `Gio.Subprocess` 启动，不经过 shell，也不阻塞菜单。
- `wait_async` 只用于回收子进程，避免僵尸进程；不会等待 GUI 退出。
- 找不到 `netcore-gui` 时，`spawnv()` 的异常被捕获并显示在状态行。

### 资源清理

`disable()` 时：

1. `NetcoreClient.stop()`：移除 5 秒轮询定时器、取消 `Gio.Cancellable`、
   取消订阅 `SnapshotChanged` 与 `NameOwnerChanged`、清除连接引用。
2. 移除通知自动清除定时器、tooltip 延时定时器。
3. 关闭仍打开的密码框，销毁 tooltip（断开 hover / 菜单信号）。
4. 销毁 PanelMenu.Button；其菜单、分区和自定义行随之销毁。
5. 所有 D-Bus/定时器回调都有 `_stopped`/`_destroyed` 守卫，避免禁用后的
   迟到回调访问已销毁 UI。

## 已知限制

- **GNOME Shell 版本**：仅支持 45–48。GNOME 44 及更早版本使用旧式 imports，
  不兼容。
- **Wayland 重载**：Wayland 下无法用 `Alt+F2 r` 重启 Shell，安装/更新扩展后
  必须注销并重新登录。这是 GNOME 会话的限制，不是本扩展能绕过的。
- **仅系统总线**：扩展按设计固定连接系统 D-Bus，不读取
  `NETCORE_BUS=session`。开发时若守护进程只运行在会话总线，请改为在系统
  总线上启动它（测试客户端本身支持显式总线地址，但扩展 UI 不启用该选项）。
- **单 Wi-Fi 设备视图**：多块 Wi-Fi 网卡时只展示主 Wi-Fi 设备的网络列表，而
  不是把所有网卡的结果合并。
- **密码框能力有限**：只提供单个密码输入框。企业级 802.1X 的用户名、证书、
  EAP 方法等无法在此配置；WEP/WPA 的密钥格式最终由守护进程校验。
- **不探测互联网**：快照的 `connectivity` 只表示是否配置了可用地址，扩展不做
  联网探测，也不显示“无互联网”门户提示。
- **不保存 Wi-Fi 密码**：密码只用于本次 `ConnectWifi` 调用，扩展不落盘。
- **polkit**：修改网络需要授权。若认证被取消，状态行显示
  “权限不足或认证被取消”，扩展无法代替 polkit 弹窗。
- **netcore-gui 路径**：必须能从 GNOME Shell 进程的 `PATH` 找到；扩展不读取
  额外配置，也不提供设置界面（按要求省略 settings-schema）。
- **测试范围**：本次在命令行下完成了 JS 语法检查、metadata 解析和基于私有
  D-Bus 的客户端集成测试；没有在真实 GNOME Shell 会话里启用扩展，因此面板
  外观和模态对话框的最终视觉效果需在桌面会话中人工确认。

## 故障排查

| 现象 | 排查 |
|---|---|
| 扩展列表里没有 netcore | 目录名必须恰好是 `netcore@freebsd.local`；`metadata.json` 的 `uuid` 必须一致；重启会话。 |
| 显示 “netcore 守护进程不可用” | 确认守护进程在系统总线：`gdbus call --system ... GetSnapshot`；检查 daemon 日志和 D-Bus 服务文件。 |
| 所有修改操作都提示权限不足 | 检查 polkit 是否正常工作，或使用 root 运行守护进程进行开发测试。 |
| 点“打开网络管理器”报找不到文件 | 安装 `netcore-gui`，或确认其路径在 Shell 的 `PATH` 中。 |
| 面板没有 tooltip | 确认 Shell 版本为 45+，并在 Looking Glass 中查看扩展是否有 JS 异常。 |
| FreeBSD 没有 gnome-extensions 命令 | 使用“复制到用户扩展目录”的方式，再用图形界面 “扩展” 应用启用。 |
| 修改代码后行为没变 | 注销/重新登录（Wayland 必须）；X11 可用 `Alt+F2 r`。 |

## 协议参考

- `docs/dbus-api.md`
- `docs/json-schema.md`
- `crates/netcore-common/src/model.rs`
- `crates/netcore-client/src/lib.rs`

扩展将协议版本视为不透明；只依赖 v1 文档中的字段，并忽略未知字段。
