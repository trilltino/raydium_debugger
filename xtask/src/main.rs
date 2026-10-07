//! Developer automation tasks for Raydium registry generation and drift checks.

mod cli;
mod http;
mod registry;
mod snapshot;
mod sources;

fn main() -> anyhow::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    cli::run(&args)
}
