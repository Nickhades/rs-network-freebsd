# netcore Plasma 6 小组件

这是 netcore 原生网络管理器的 KDE Plasma 6 面板小组件（Plasma Applet）。它直接通过系统总线与
`org.netcore.NetworkManager` 通信，不轮询、不启动 `netcore-cli`，也不依赖 netcore 的 Rust 代码。

- 面板紧凑表示：网络状态符号图标，悬浮提示显示 SSID / 状态 / IP。
- 完整表示：Wi-Fi 开关、刷新、扫描、Wi-Fi 列表（按信号强度排序，含安全类型、信号百分比、锁定图标）、
  连接/断开、密码输入、设备列表（启用开关、以太网连接/断开）、启动 `netcore-gui`。
- UI 全部为中文，使用 PlasmaComponents3 / Kirigami / PlasmaExtras，符合 Plasma 6 原生外观。
- 所有 D-Bus 调用都是异步的，不阻塞 plasmashell 的 GUI 线程。

## 目录结构

```text
plugins/kde/netcore/
├── metadata.json                      # Plasma/Applet 元数据
├── CMakeLists.txt                     # 推荐构建方式
├── Makefile                           # build.sh 的简单包装
├── build.sh                           # 无 CMake 时的构建 / 检查 / 安装脚本
├── README.md
├── .gitignore
├── contents/ui/
│   ├── main.qml                       # PlasmoidItem，持有唯一的 backend 实例
│   ├── CompactRepresentation.qml      # 面板图标
│   ├── FullRepresentation.qml         # 弹出面板完整 UI
│   └── PasswordDialog.qml             # 原生密码输入框
├── src/
│   ├── netcorebackend.h
│   ├── netcorebackend.cpp             # QtDBus 异步桥接、JSON 解析
│   ├── netcoreplugin.h
│   └── netcoreplugin.cpp              # Netcore.Backend 1.0 QQmlExtensionPlugin
└── qml/
    ├── qmldir
    └── netcorebackend.qmltypes        # qmllint / 工具链用的类型信息
```

## 构建依赖

在 FreeBSD 上需要的运行时/构建组件：

- Qt 6（本机验证版本 6.11.2）：Core、DBus、Qml 开发文件。
- Plasma 6（本机验证版本 6.7.5）的运行时 QML 模块：
  `org.kde.plasma.plasmoid`、`org.kde.plasma.components`、`org.kde.plasma.extras`、
  `org.kde.kirigami`、`org.kde.plasma.core`。
- 构建 C++ 插件只需 Qt6；不需要链接 libplasma / PlasmaQuick 的 C++ 头文件。
  本机检查到 KF6 Plasma 开发库（Plasma、PlasmaQuick、ECM）是存在的，但插件不依赖它们，
  这样构建面更小，也不会被特定 Plasma 小版本锁定。
- CMake 方式需要 CMake ≥ 3.16 和 Ninja（或 Make）；`build.sh` 方式只需要 `qmake6`、
  `moc`、`pkg-config` 和 C++17 编译器。

FreeBSD 典型安装：

```sh
# 软件包名称随仓库可能略有不同，按实际 pkg 输出为准
pkg install cmake ninja qt6-base qt6-declarative qt6-tools plasma6-plasma-workspace plasma6-plasma-desktop
```

## 构建与安装

### 方式一：CMake（推荐）

```sh
cd plugins/kde/netcore
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build
sudo cmake --install build
```

默认安装位置（FreeBSD，`CMAKE_INSTALL_PREFIX=/usr/local`）：

- QML 模块：`/usr/local/lib/qt6/qml/Netcore/Backend/`
  - `libnetcorebackendplugin.so`
  - `qmldir`
  - `netcorebackend.qmltypes`
- Plasma 小程序：`/usr/local/share/plasma/plasmoids/org.netcore.networkmanager/`
  - `metadata.json`
  - `contents/ui/*.qml`

QML 模块路径从 Qt 自身推导（`QT6_INSTALL_QML` / `QT6_INSTALL_PREFIX`），不是从
`CMAKE_INSTALL_PREFIX` 推导；如 Qt 装在别处，可显式覆盖：

```sh
cmake -S . -B build -G Ninja \
  -DNETCORE_QML_INSTALL_DIR=/usr/local/lib/qt6/qml/Netcore/Backend
cmake --build build
sudo cmake --install build
```

打包暂存（DESTDIR 由 CMake 安装规则自动支持）：

```sh
DESTDIR=/tmp/netcore-stage cmake --install build
```

### 方式二：build.sh / Makefile（本机实际验证的方式）

本机没有安装 `cmake` 可执行文件，因此实际验证使用这个脚本。它做与 CMake 相同的两件事：
编译 `Netcore.Backend` QML 插件，并把插件和 Plasma 包安装到正确路径。

```sh
cd plugins/kde/netcore

# 构建
./build.sh --build

# 构建 + JSON/qmllint/qmlcachegen/qmlformat 检查
./build.sh --check

# 安装（默认 /usr/local）
sudo ./build.sh --install

# 安装到暂存目录，用于打包
DESTDIR=/tmp/netcore-stage PREFIX=/usr/local ./build.sh --install

# 如果 Qt 的 QML 目录不是默认值
sudo QML_DIR=/usr/local/lib/qt6/qml ./build.sh --install

# 清理
./build.sh --clean
```

Makefile 只是包装：

```sh
make            # 构建
make check      # 检查
sudo make install
sudo make install DESTDIR=/tmp/netcore-stage
make clean
```

`build.sh` 支持的变量：`PREFIX`、`DESTDIR`、`QML_DIR`、`BUILD_DIR`、`CXX`、`QMAKE`、
`MOC`、`QMLLINT`、`QMLCACHEGEN`、`QMLFORMAT`。

## 安装后使用

1. 启动 netcore 守护进程，确认它在系统总线上：
   ```sh
   gdbus call --system --dest org.netcore.NetworkManager \
     --object-path /org/netcore/NetworkManager \
     --method org.netcore.NetworkManager.GetSnapshot
   ```
2. 让 plasmashell 重新加载 QML 缓存/新插件。最稳妥的是注销后重新登录 Plasma 会话；
   在 Plasma 会话内也可以：
   ```sh
   kquitapp6 plasmashell
   plasmashell --replace &
   ```
3. 右键面板 → “添加小组件” → 搜索 **netcore 网络管理器**，或
   `kpackagetool6 --type Plasma/Applet --list` 确认包已安装。

注意：本文件所在开发环境运行的是 XFCE，验证过程**没有**安装到当前会话、也没有重启
plasmashell；安装与加载需要在 Plasma 6 会话中进行。

## 运行时假设

- 守护进程名为 `org.netcore.NetworkManager`，对象路径 `/org/netcore/NetworkManager`，
  接口同名；所有数据是 docs/dbus-api.md 与 docs/json-schema.md 描述的 v1 JSON。
- 读方法本地可用；写方法由守护进程通过 polkit 的 `org.netcore.manage` 动作检查，
  可能需要桌面 polkit 认证代理。认证失败会以可读错误显示在完整表示的错误条中。
- `netcore-gui` 需要在 `PATH` 中，或位于 `/usr/local/bin/netcore-gui`。
- Plasma 6.0+（metadata 要求 `X-Plasma-API-Minimum-Version: 6.0`），Qt 6.5+；
  本机组合为 FreeBSD 15.1 + Qt 6.11.2 + Plasma 6.7.5。
- C++ 插件使用 Qt 的 QML 插件 ABI。升级 Qt 后需要重新编译并安装插件。

## 实现说明

### QML / C++ 分工

`main.qml` 的根 `PlasmoidItem` 中创建一个 `NetcoreBackend` 对象，紧凑表示和完整表示都只
通过 `required property NetcoreBackend backend` 借用它。这样：

- D-Bus 信号订阅和 15 秒兜底刷新在弹出面板关闭后仍然存在；
- 同一个 applet 的两套表示不会各自创建连接；
- applet 被删除时，QML 对象树负责销毁 backend 和所有 `QDBusPendingCallWatcher`。

### 后端 API

QML 可读写属性：

| 属性 | 类型 | 说明 |
|---|---|---|
| `snapshot` | QVariantMap | 解析并补充过字段的完整快照 |
| `devices` | QVariantList | 已过滤回环设备、带中文标签/IP/图标等字段 |
| `wifiDevices` | QVariantList | Wi-Fi 接口列表 |
| `wifiNetworks` | QVariantList | 当前 Wi-Fi 接口的网络，按信号降序 |
| `wifiDevice` | QString | 当前选中的 Wi-Fi ifname（可写） |
| `daemonAvailable` | bool | 守护进程是否可用 |
| `busy` | bool | 是否有写操作/扫描在途 |
| `lastError` | QString | 最近一次可显示错误 |
| `state`, `stateLabel`, `connectivityLabel` | QString | 管理器状态及中文标签 |
| `activeDevice`, `activeKind`, `activeName`, `activeSsid`, `activeIpv4` | QString | 活动连接信息 |
| `hostname`, `daemonVersion`, `wpaBackend`, `netlinkActive` | 混合 | 状态栏/诊断信息 |
| `wifiEnabled`, `wifiAvailable`, `hasWifiDevice` | bool | Wi-Fi 开关与硬件状态 |
| `compactIcon`, `toolTipTitle`, `toolTipSubText` | QString | 面板图标和悬浮提示 |

QML 可调用方法：

```text
refresh()
scan()
connectWifi(ifname, ssid, password, hidden)
disconnectDevice(ifname)
setDeviceEnabled(ifname, enabled)
setWifiEnabled(enabled)
openGui()
clearError()
setWifiDevice(ifname)
```

对应 D-Bus 方法与 docs/dbus-api.md 完全一致：`GetSnapshot`、`ScanWifi`、`ConnectWifi`、
`DisconnectDevice`、`SetDeviceEnabled`、`SetWifiEnabled`。`openGui()` 使用
`QProcess::startDetached` 启动 `netcore-gui`，不经过命令行轮询。

### 为什么不用 QDBusInterface

`QDBusInterface` 的构造函数会同步执行 `Introspect`；守护进程未启动、变慢或系统总线异常时，
这会阻塞 plasmashell 的 GUI 线程。因此后端改用：

```cpp
QDBusMessage message = QDBusMessage::createMethodCall(...);
const QDBusPendingCall pending = bus.asyncCall(message, timeoutMs);
auto *watcher = new QDBusPendingCallWatcher(pending, this);
connect(watcher, &QDBusPendingCallWatcher::finished, ...);
```

所有回复（包括错误、超时）都通过 `QDBusPendingCallWatcher::finished` 回到 GUI 线程；
`watcher->deleteLater()` 释放，不出现悬挂指针。信号订阅包括 `SnapshotChanged`、
`NetworkScanFinished`、`Error`，并用 `QDBusServiceWatcher` 处理守护进程启动/退出。
15 秒定时器只是信号丢失时的兜底，不是 `netcore-cli` 轮询。

### JSON 与展示字段

- 严格按 v1 schema 读取；未知字段忽略，缺失字段使用安全默认值。
- `devices` 中回环设备不展示；空 SSID 的隐藏 BSS 不进入列表，隐藏网络通过“连接到隐藏网络…”入口。
- Wi-Fi 列表按 `signal` 降序排序；`quality = clamp((clamp(signal,-100,-50)+100)*2,0,100)`，
  与 netcore-common 的 `quality_percent()` 一致。
- 安全类型映射为中文：开放/WEP/WPA/WPA2/WPA3/企业级/未知；
  `open` 与 `unknown` 不需要密码，其余类型点击时打开密码框。
- 图标使用 Breeze 的 symbolic 名称，例如
  `network-wireless-connected-100-symbolic`、
  `network-wireless-signal-excellent-symbolic`、`lock-symbolic`、
  `network-wired-symbolic`、`network-offline-symbolic`。

### 错误与状态

- 守护进程不可用：完整表示显示 `PlaceholderMessage`，面板图标变为 `network-offline-symbolic`，
  并在错误/提示中说明 `org.netcore.NetworkManager`。
- 方法返回 D-Bus 错误：错误条显示“操作：消息”，可点 × 关闭；`busy` 在写操作期间为 true。
- 连接/断开/开关成功后会主动 `refresh()`；`SnapshotChanged` 仍是主要更新来源。
- 密码只在内存中传给 D-Bus，不写入配置；QML 在提交后立即清空输入框。

## 验证记录

以下验证均在本机执行，未安装到当前 XFCE 会话、未重启 plasmashell。

```text
主机: FreeBSD 15.1-RELEASE-p7 amd64
Qt:   6.11.2
Plasma: 6.7.5
CMake 可执行文件: 未安装（因此 CMake 构建规则未实际执行，改用 build.sh 验证）
```

1. **JSON**：`python3 -m json.tool metadata.json` 通过；`kpackagetool6 --type Plasma/Applet`
   安装到临时 packageroot 成功并可列出 `org.netcore.networkmanager`；
   `--appstream-metainfo` 能正确解析出 Id/Name/Description/Icon/License。
2. **C++ 插件**：用 `moc` + `c++ -shared` + pkg-config Qt6Core/Qt6DBus/Qt6Qml 构建成功；
   `./build.sh --check` 退出码 0。
3. **qmllint**（Qt 6.11.2）：所有 4 个 QML 文件解析通过，退出码 0，无警告。
   最初 qmllint 对 qmltypes 中的 `Q_PROPERTY QString` 报过 4 条
   `incompatible-type` 保守提示，已用显式 `String(...)` 转换消除；
   未限定访问等警告则通过 `pragma ComponentBehavior: Bound` 和限定 id 消除。
4. **qmlcachegen**：`main.qml`、`CompactRepresentation.qml`、`FullRepresentation.qml`、
   `PasswordDialog.qml` 全部编译通过，生成 `.qmlc`。
5. **qmlformat**：4 个文件均可被 qmlformat 解析（仅作为语法检查；未采用其默认格式化，
   因为它会把注释缩进和长表达式改成不理想的样式）。
6. **无守护进程冒烟测试**：`QT_QPA_PLATFORM=offscreen` 加载插件并实例化 backend，
   2 秒内正常结束；`daemonAvailable=false`，`lastError` 为
   `netcore 守护进程未运行（org.freedesktop.DBus.Error.ServiceUnknown）`，GUI 未阻塞。
7. **模拟守护进程集成测试**：启动私有 `dbus-daemon`，用 Python dbus 模拟
   `org.netcore.NetworkManager`，验证：
   - 快照解析：`activeName=HomeNet`、`activeIpv4=192.168.2.226`、Wi-Fi=3、设备=2（回环已过滤）；
   - 排序：HomeNet(-48) → OpenNet(-55) → WeakNet(-82)；
   - 安全/信号字段：WPA3/开放/WPA2、quality 100/90/36、锁定图标字段正确；
   - 异步调用：`ScanWifi wlan0`、`ConnectWifi wlan0 OpenNet`、`SetDeviceEnabled em0` 都被模拟服务收到；
   - 最终 `busy=false`、`daemonAvailable=true`、无错误。
8. **完整表示 / 密码框冒烟测试**：offscreen 加载 `FullRepresentation.qml` 并连接模拟守护进程，
   读到 `networks=3`、`devices=2`；`PasswordDialog.openFor()` 与 `openHidden()` 的标题和可见性正确；
   未出现 binding loop、类型错误或布局报错。
9. **未执行项**：CMake 构建、真实 plasmashell 加载、polkit 实际认证、真实 netcore 守护进程连接、
   `netcore-gui` 启动。这些需要 Plasma 会话 / 守护进程 / 认证代理。

## 故障排查

**QML 报 “module `Netcore.Backend` is not installed”**

- 确认 `/usr/local/lib/qt6/qml/Netcore/Backend/libnetcorebackendplugin.so`、`qmldir`、
  `netcorebackend.qmltypes` 都在。
- 确认安装到了 `qmake6 -query QT_INSTALL_QML` 对应的目录，而不是只安装 applet 包。
- Qt 大版本升级后重新编译插件；ABI 不匹配会导致插件加载失败。
- 调试：`QT_DEBUG_PLUGINS=1 plasmashell --replace` 查看插件加载日志。

**面板显示 “netcore 未运行”**

- `gdbus call --system ... GetSnapshot` 检查系统总线上的名称。
- 确认 netcore 守护进程以提供 `/org/netcore/NetworkManager` 的方式启动；
  开发模式如果使用会话总线，需要让插件也连接到相同的总线（当前插件固定使用系统总线）。

**点击连接后出现 “未授权” / polkit 提示**

- 守护进程通过 polkit 的 `org.netcore.manage` 动作检查写操作；需要 polkit 认证代理。
  读操作（快照、扫描）不需要授权。
- 可先用 `pkaction --action-id org.netcore.manage --verbose` 检查策略是否安装。

**找不到 netcore-gui**

- 把 `netcore-gui` 放进 `PATH`，或安装到 `/usr/local/bin/netcore-gui`。

**安装路径 / DESTDIR 打包**

- QML 模块安装目录由 `QML_DIR`（或 CMake 的 `NETCORE_QML_INSTALL_DIR`）控制；
  小程序安装目录为 `${PREFIX}/share/plasma/plasmoids/org.netcore.networkmanager`。
- 打包时使用 `DESTDIR=/tmp/stage`，目录会变成
  `/tmp/stage/usr/local/lib/qt6/qml/Netcore/Backend` 和
  `/tmp/stage/usr/local/share/plasma/plasmoids/org.netcore.networkmanager`。

## 许可

BSD-2-Clause，与 rs-network-freebsd 项目一致。
