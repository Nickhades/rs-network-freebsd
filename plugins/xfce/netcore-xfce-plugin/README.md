# netcore XFCE Panel 插件

这是一个用 Rust 编写的原生 XFCE Panel 4.20 插件，直接通过 GDBus 与
netcore 守护进程通信，不轮询子进程，也不阻塞面板主循环。

## 构建

    cargo build --release -p netcore-xfce-plugin

产物为 target/release/libnetcore_xfce_plugin.so。

## 安装

系统安装（需要 root）：

    install -d /usr/local/lib/xfce4/panel/plugins
    install -m 755 target/release/libnetcore_xfce_plugin.so \
      /usr/local/lib/xfce4/panel/plugins/libnetcore_xfce_plugin.so
    install -d /usr/local/share/xfce4/panel/plugins
    install -m 644 plugins/xfce/netcore-xfce-plugin/netcore-plugin.desktop \
      /usr/local/share/xfce4/panel/plugins/netcore-plugin.desktop
    xfce4-panel -q; xfce4-panel &

开发会话只安装到用户目录时，把 .so 放到 ~/.local/lib/xfce4/panel/plugins，
.desktop 放到 ~/.local/share/xfce4/panel/plugins，然后重启面板。插件进程
会自动读取 NETCORE_BUS=session 以连接开发用的会话总线；正常安装使用系统
总线。

## 添加

面板右键 -> 面板 -> 添加新项目 -> “netcore Network”。左键或右键插件图标
打开菜单。

## 注意

- 插件运行在 xfce4-panel 进程内，网络操作全部异步执行。
- 需要守护进程已安装 D-Bus 服务文件，或由 rc.d 启动。否则图标显示“不可用”。
- 连接企业级/WEP 网络会收到明确的不支持错误。
