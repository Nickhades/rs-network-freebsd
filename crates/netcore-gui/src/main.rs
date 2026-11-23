//! GTK4 network manager frontend for netcore.

mod app;
mod client;

use clap::{Parser, ValueEnum};
use netcore_client::BusKind;

#[derive(Debug, Parser)]
#[command(name = "netcore-gui", version, about = "netcore 图形网络管理器")]
struct Args {
    /// 使用 system 或 session D-Bus（也可设置 NETCORE_BUS）。
    #[arg(long, value_enum)]
    bus: Option<BusArg>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BusArg {
    System,
    Session,
}

fn main() {
    let args = Args::parse();
    let bus = args.bus.map_or_else(BusKind::from_env, |bus| match bus {
        BusArg::System => BusKind::System,
        BusArg::Session => BusKind::Session,
    });
    app::run(bus);
}
