# 安装

netcore 提供两种一键安装入口：

| 入口 | 行为 | 需要编译器 |
|---|---|---|
| scripts/install-freebsd.sh | 自动检测编译环境；可编译则编译，否则尝试安装预编译产物 | 视环境而定 |
| scripts/install-freebsd-prebuilt.sh | 只安装已有编译产物，绝不调用 cargo/qmake/cc | 不需要 |

两者最终都执行相同的文件安装：守护进程、CLI、GTK4 界面、XFCE 插件、
GNOME 扩展、可选 KDE 集成、rc.d、D-Bus、polkit 和桌面文件。

## 自动模式

    sudo ./scripts/install-freebsd.sh

默认模式是 auto。检测顺序：

1. 检查 cargo、rustc、C 编译器、pkg-config、GTK4、GTK3、
   libxfce4panel-2.0。
2. 如果完整：运行
       cargo build --release --workspace --locked
   然后安装。Qt6 构建环境完整时额外构建 KDE Plasma 插件；环境缺失但已有
   预编译插件时使用之，否则跳过。
3. 如果 Rust/GTK 环境不完整：检查 target/release 中四个核心产物。
   - 齐全：改用预编译安装；
   - 不齐全：打印缺失清单并退出，不做半安装。
4. 如果 KDE 构建失败，auto 会尝试已有 KDE 预编译插件；没有则跳过 KDE，
   核心组件仍会安装。

KDE 构建目录使用 target/kde-qml，不写入源码树内的 build 目录，避免
root 安装后留下不可写的目录。

## 预编译模式

    sudo ./scripts/install-freebsd-prebuilt.sh

这个入口强制 --binary。它只检查并复制以下产物：

    target/release/netcore
    target/release/netcore-gui
    target/release/netcore-daemon
    target/release/libnetcore_xfce_plugin.so
    plugins/kde/netcore/build/Netcore/Backend/libnetcorebackendplugin.so
    或 target/kde-qml/Netcore/Backend/libnetcorebackendplugin.so

预编译模式不会运行 cargo、qmake、moc 或 C/C++ 编译器。缺少 KDE 插件时
会跳过 KDE 集成，其余组件照常安装。为了在无编译环境的目标机上获得完整
KDE 集成，请把预编译 QML 插件一并带到目标机。

在源码机上手工构建后使用预编译模式，可以避免 sudo 构建产生 root 所有的
target 文件：

    cargo build --release --workspace --locked
    (cd plugins/kde/netcore && ./build.sh --build)
    sudo ./scripts/install-freebsd-prebuilt.sh

注意：当前预编译模式依赖仓库中的安装数据文件和 ./target 产物，不是单独
自包含的二进制 tar 包。制作自包含发布包时，应把这两个脚本、packaging/、
plugins/ 与 target/release 一同打包（后续可增加 release bundle 目标）。

## 环境检查

不编译、不安装，只查看自动模式会做什么：

    ./scripts/install-freebsd.sh --check

示例输出包含 Rust/GTK 环境、KDE 环境、预编译产物是否存在，以及 auto
模式将编译、回退或失败。

## 强制模式

    sudo ./scripts/install-freebsd.sh --source
    sudo ./scripts/install-freebsd.sh --binary

--source 不会回退到预编译产物；编译环境不完整会直接失败。
--binary 等价于预编译入口。

## 暂存安装与自定义前缀

设置 DESTDIR 可以非 root 安装到临时根目录，适合打包、jail 和 CI：

    DESTDIR=/tmp/netcore-stage ./scripts/install-freebsd.sh
    DESTDIR=/tmp/netcore-stage ./scripts/install-freebsd-prebuilt.sh

自定义 PREFIX：

    PREFIX=/opt/netcore DESTDIR=/tmp/netcore-stage ./scripts/install-freebsd.sh

服务文件、D-Bus service 与 GUI .desktop 的 Exec 路径按 /usr/local 编写；
使用其他 PREFIX 时需要同步修改，否则系统启动时会找错路径。

## 安装后

    sysrc netcore_enable=YES
    service dbus restart
    service netcore start
    service netcore status
    netcore status

如果只是想更新二进制而不重启服务，替换文件后执行：

    service netcore restart

D-Bus policy 文件变更后需要重启 dbus 或系统。

## 故障排查

- auto 打印“编译环境不完整”：安装 rust、gtk4、xfce4-panel 开发文件与
  pkgconf，或使用预编译入口。
- auto 打印“预编译产物缺少”：先在没有 sudo 的用户下执行
  cargo build --release --workspace，或改用 --source 完整构建。
- KDE 被跳过：安装 Qt6 base/declarative/tools 开发文件后重新运行 auto。
- 提示需要 root：直接安装 /usr/local 需要 sudo；也可以用 DESTDIR 暂存。
- 旧安装残留：脚本只覆盖已知路径，不会删除旧版本额外文件；需要彻底清理时
  对照 packaging/freebsd/pkg-plist 删除。