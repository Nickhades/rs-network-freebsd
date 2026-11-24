//! GTK4 network manager frontend for netcore.

mod accel;
mod app;
mod client;
mod icons;
mod theme;

use clap::{Parser, ValueEnum};
use netcore_client::BusKind;
use theme::ThemeMode;

#[derive(Debug, Parser)]
#[command(name = "netcore-gui", version, about = "netcore 图形网络管理器")]
struct Args {
    /// 使用 system 或 session D-Bus（也可设置 NETCORE_BUS）。
    #[arg(long, value_enum)]
    bus: Option<BusArg>,

    /// 启动主题。默认深色；界面内也可以随时切换。
    #[arg(long, value_enum, default_value_t = ThemeArg::Dark)]
    theme: ThemeArg,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BusArg {
    System,
    Session,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ThemeArg {
    Dark,
    Light,
}

fn main() {
    let args = Args::parse();
    let bus = args.bus.map_or_else(BusKind::from_env, |bus| match bus {
        BusArg::System => BusKind::System,
        BusArg::Session => BusKind::Session,
    });
    let theme = match args.theme {
        ThemeArg::Dark => ThemeMode::Dark,
        ThemeArg::Light => ThemeMode::Light,
    };
    accel::prepare_renderer();
    app::run(bus, theme);
}
