//! netcore-daemon: privileged network manager daemon for FreeBSD.

mod auth;
mod config;
mod dbus;
mod dhcp;
mod error;
mod iface;
mod manager;
mod netlink;
mod system;
mod wifi;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use config::{BusChoice, Config};
use manager::Manager;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use crate::auth::Authorizer;
use crate::dbus::DaemonService;
use crate::wifi::WifiService;

fn main() {
    let config = Config::parse();
    init_logging(&config);
    if config.netlink_helper {
        if let Err(error) = netlink::capsicum_helper_main() {
            eprintln!("netlink helper: {error:#}");
            std::process::exit(1);
        }
        return;
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("netcore")
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("无法创建异步运行时: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = runtime.block_on(run(config)) {
        tracing::error!("daemon 退出: {error:#}");
        eprintln!("netcore-daemon: {error:#}");
        std::process::exit(1);
    }
}

fn init_logging(config: &Config) {
    let filter = EnvFilter::try_new(config.log_filter())
        .unwrap_or_else(|_| EnvFilter::new("netcore_daemon=info,zbus=warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true)
        .try_init();
}

async fn run(config: Config) -> Result<()> {
    let wifi = WifiService::detect(
        config.wpa_backend,
        config.wpa_ctrl_dir.clone(),
        config.wpa_cli.clone(),
    )
    .await;
    tracing::info!(
        backend = wifi.kind(),
        available = wifi.available(),
        "初始化 Wi-Fi 后端"
    );

    let manager = Manager::new(&config, wifi).await;
    let connection = match config.bus {
        BusChoice::System => zbus::Connection::system().await,
        BusChoice::Session => zbus::Connection::session().await,
    }
    .context("无法连接 D-Bus")?;
    let service = DaemonService::new(
        manager.clone(),
        Authorizer::new(config.auth, config.bus, config.allow_group.clone()),
    );
    let replaced = connection
        .object_server()
        .at(netcore_common::DBUS_PATH, service)
        .await
        .context("注册 D-Bus 对象失败")?;
    if replaced.is_some() {
        bail!("D-Bus 对象路径已被占用");
    }
    dbus::acquire_name(&connection).await?;

    let signal_task = tokio::spawn(dbus::signal_pump(connection.clone(), manager.subscribe()));

    let (events_tx, mut events_rx) = mpsc::channel(64);
    let active = manager.netlink_flag();
    let netlink_result = if config.capsicum {
        netlink::spawn_capsicum_helper(events_tx, Arc::clone(&active)).await
    } else {
        netlink::spawn_in_process(events_tx, Arc::clone(&active)).await
    };
    if let Err(error) = netlink_result {
        active.store(false, std::sync::atomic::Ordering::SeqCst);
        tracing::warn!(%error, "Netlink 监听不可用，将依赖周期轮询");
    }

    let event_manager = manager.clone();
    let event_task = tokio::spawn(async move {
        while events_rx.recv().await.is_some() {
            // Coalesce bursts from netlink into one snapshot.
            tokio::time::sleep(Duration::from_millis(120)).await;
            while events_rx.try_recv().is_ok() {}
            event_manager.refresh().await;
        }
    });

    let poll_manager = manager.clone();
    let poll_interval = Duration::from_secs(config.poll_interval.max(2));
    let poll_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(poll_interval).await;
            poll_manager.refresh().await;
        }
    });

    tracing::info!(
        bus = ?config.bus,
        capsicum_helper = config.capsicum,
        privileged = system::is_root(),
        "netcore-daemon 已就绪"
    );
    if !system::is_root() {
        tracing::warn!("守护进程未以 root 运行，网络修改操作将返回 NotAuthorized");
    }
    manager.refresh().await;

    wait_for_shutdown().await?;
    tracing::info!("收到退出信号，正在停止");
    manager.shutdown().await;
    signal_task.abort();
    event_task.abort();
    poll_task.abort();
    Ok(())
}

async fn wait_for_shutdown() -> Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).context("注册 SIGTERM 失败")?;
        let mut interrupt = signal(SignalKind::interrupt()).context("注册 SIGINT 失败")?;
        tokio::select! {
            _ = terminate.recv() => {},
            _ = interrupt.recv() => {},
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.context("等待 Ctrl-C 失败")?;
        Ok(())
    }
}
