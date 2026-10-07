//! Local Axum server entrypoint for the web debugger.

mod app;
mod error;
use raydium_debugger_server::investigation;
mod review_routes;
mod routes;
mod state;
mod store;

use clap::Parser;
use std::{net::SocketAddr, sync::Arc};

/// Local Axum server for the debugger API and built React app.
#[derive(Parser, Debug)]
#[command(name = "raydium-debugger-server")]
#[command(about = "Serve the Raydium debugger web UI and API")]
struct Args {
    /// Socket address used by the local HTTP server.
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: SocketAddr,
}

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(run());
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    result
}

async fn run() -> anyhow::Result<()> {
    dotenvy::from_filename(".env.local").ok();
    dotenvy::dotenv().ok();
    raydium_investigation::init_tracing();
    let args = Args::parse();
    ensure_local_bind(args.bind)?;
    let listener = tokio::net::TcpListener::bind(args.bind).await?;

    println!("raydium-debugger-server listening on http://{}", args.bind);
    let state = Arc::new(tokio::task::spawn_blocking(state::AppState::from_env).await??);
    let service = state.investigation_service.clone();
    let router = app::router_with_state(state);
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, router).with_graceful_shutdown(async {
        let _ = stopped.await;
    });
    let server = std::future::IntoFuture::into_future(server);
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result?,
        _ = tokio::signal::ctrl_c() => {
            let _ = stop.send(());
            let drain = service.shutdown();
            tokio::pin!(drain);
            let _ = tokio::time::timeout(std::time::Duration::from_secs(15), async { let _ = tokio::join!(&mut server, &mut drain); }).await;
        }
    }
    Ok(())
}

fn ensure_local_bind(bind: SocketAddr) -> anyhow::Result<()> {
    if bind.ip().is_loopback() {
        return Ok(());
    }

    anyhow::bail!(
        "refusing to bind local-token API to non-loopback address {bind}; use an operator-controlled tunnel for remote access"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_bind_is_allowed() {
        let bind: SocketAddr = "127.0.0.1:8787".parse().unwrap();
        assert!(ensure_local_bind(bind).is_ok());
    }

    #[test]
    fn public_bind_is_always_rejected() {
        let bind: SocketAddr = "0.0.0.0:8787".parse().unwrap();
        assert!(ensure_local_bind(bind).is_err());
    }
}
