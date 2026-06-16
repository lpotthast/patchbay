#![recursion_limit = "256"]

mod backend;
mod frontend;
mod shared;
mod tracing_init;

use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;
use rootcause::Result;

use crate::backend::{
    server,
    storage::{Store, default_database_path},
};

#[derive(Debug, Parser)]
#[command(name = "patchbay-server")]
#[command(about = "Patchbay server")]
struct ServerArgs {
    #[arg(long, env = "PATCHBAY_DATABASE")]
    database: Option<PathBuf>,

    #[arg(long, default_value_t = default_bind_addr())]
    bind: SocketAddr,
}

fn default_bind_addr() -> SocketAddr {
    std::env::var("LEPTOS_SITE_ADDR")
        .or_else(|_| std::env::var("PATCHBAY_BIND"))
        .unwrap_or_else(|_| "127.0.0.1:4000".to_owned())
        .parse()
        .expect("LEPTOS_SITE_ADDR or PATCHBAY_BIND must be a valid socket address")
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_init::init();

    let args = ServerArgs::parse();
    let database = args.database.unwrap_or_else(default_database_path);
    tracing::info!(path = %database.display(), "Database path");
    let store = Store::open(database).await?;

    tracing::info!(url = %format_args!("http://{}", args.bind), "Starting Patchbay");
    server::serve(store, args.bind).await
}
