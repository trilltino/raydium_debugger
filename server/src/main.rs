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
    /// Allow binding the local-token server to a non-loopback interface.
    #[arg(long)]
    allow_non_loopback: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::from_filename(".env.local").ok();
    dotenvy::dotenv().ok();
    ensure_triton_configured()?;
    let args = Args::parse();
    ensure_local_bind(args.bind, args.allow_non_loopback)?;
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

fn ensure_local_bind(bind: SocketAddr, allow_non_loopback: bool) -> anyhow::Result<()> {
    if allow_non_loopback || bind.ip().is_loopback() {
        return Ok(());
    }

    anyhow::bail!(
        "refusing to bind local-token API to non-loopback address {bind}; use --allow-non-loopback only behind a trusted network boundary"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_bind_is_allowed() {
        let bind: SocketAddr = "127.0.0.1:8787".parse().unwrap();
        assert!(ensure_local_bind(bind, false).is_ok());
    }

    #[test]
    fn public_bind_requires_explicit_opt_in() {
        let bind: SocketAddr = "0.0.0.0:8787".parse().unwrap();
        assert!(ensure_local_bind(bind, false).is_err());
        assert!(ensure_local_bind(bind, true).is_ok());
    }
}
