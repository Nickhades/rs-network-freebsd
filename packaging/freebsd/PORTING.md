# FreeBSD Ports 打包说明

本目录包含可直接使用的本地安装脚本，以及 port/ 下的 Ports 骨架。

## 本地安装

    ./scripts/install-freebsd.sh

或暂存到 DESTDIR：

    DESTDIR=/tmp/netcore-stage PREFIX=/usr/local ./scripts/install-freebsd.sh

## Port 骨架

port/ 下的 Makefile 已包含 Cargo.lock 生成的 CARGO_CRATES 列表。它尚未提交
到 FreeBSD Ports 树，也未在本环境验证，因为主机没有完整的 /usr/ports/Mk。

使用前：

1. 在 port/Makefile 中填写真实的 GH_ACCOUNT/GH_PROJECT 或 MASTER_SITES，
   并填写 MAINTAINER/WWW。
2. 检查 FreeBSD 当前的 Qt6 / GTK / XFCE port 名称与选项。
3. 将 port/ 复制为 /usr/ports/net-mgmt/netcore。
4. 用 ports 框架运行 make makesum、make stage、poudriere testport。
5. 如果 Cargo.lock 变化，重新生成 CARGO_CRATES。可由维护脚本完成：

    cargo metadata --format-version 1 --locked |       jq -r '.packages[] | select(.source != null) | "\(.name)-\(.version)"' |       sort | awk '{...}'

   FreeBSD Ports 也可以运行 make cargo-crates（取决于 ports 树版本）。

## 安装后的服务

    sysrc netcore_enable=YES
    service dbus restart
    service netcore start

D-Bus 服务文件按 /usr/local 前缀编写；变更 PREFIX 时要同步修改 Exec 路径。
