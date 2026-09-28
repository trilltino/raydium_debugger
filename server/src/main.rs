//! Local Axum server entrypoint for the web debugger.

mod app;
mod error;
mod routes;
mod state;
mod store;

use clap::Parser;
use std::net::SocketAddr;

/// Local Axum server for the debugger API and built React app.
#[derive(Parser, Debug)]
#[command(name = "raydium-debugger-server")]
#[command(about = "Serve the Raydium debugger web UI and API")]
struct Args {
    /// Socket address used by the local HTTP server.
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: SocketAddr,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::from_filename(".env.local").ok();
    dotenvy::dotenv().ok();
    ensure_triton_configured()?;
    let args = Args::parse();
    let listener = tokio::net::TcpListener::bind(args.bind).await?;

    println!("raydium-debugger-server listening on http://{}", args.bind);
    axum::serve(listener, app::router()?).await?;
    Ok(())
}

fn ensure_triton_configured() -> anyhow::Result<()> {
    let devnet = triton_env_is_set("TRITON_DEVNET_RPC_URL");
    let mainnet = triton_env_is_set("TRITON_MAINNET_RPC_URL");
    if devnet || mainnet {
        return Ok(());
    }

    anyhow::bail!(
        "Triton RPC is not configured. Set TRITON_DEVNET_RPC_URL and TRITON_MAINNET_RPC_URL in .env.local, then restart with `just dev`."
    );
}

fn triton_env_is_set(key: &str) -> bool {
    std::env::var(key).is_ok_and(|value| !value.trim().is_empty())
}
