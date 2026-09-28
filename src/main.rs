//! Command-line entrypoint for transaction debugging.

#[cfg(feature = "ai")]
use anyhow::Context;
use clap::Parser;
#[cfg(feature = "ai")]
use raydium_debugger::{run_ai_request, AiAskRequest};
use raydium_debugger::{run_debug_request, DebugDataMode, DebugRequest, RpcCluster};
use solana_sdk::signature::Signature;

#[derive(Parser, Debug)]
#[command(name = "raydium-debugger")]
#[command(about = "Debug Solana/Raydium transactions by signature")]
struct Args {
    #[arg(short, long)]
    signature: Signature,

    #[arg(long)]
    no_fallback: bool,

    #[arg(long)]
    cluster: Option<String>,

    #[arg(long)]
    data_mode: Option<String>,

    #[arg(long)]
    json: bool,

    #[arg(long)]
    ask: Option<String>,

    #[arg(long, env = "RAYDIUM_DEBUGGER_AI_MODEL")]
    ai_model: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::from_filename(".env.local").ok();
    dotenvy::dotenv().ok();
    let args = Args::parse();
    let response = run_debug_request(DebugRequest {
        signature: args.signature.to_string(),
        rpc_url: None,
        no_fallback: args.no_fallback,
        cluster: parse_cluster(args.cluster.as_deref())?,
        data_mode: parse_data_mode(args.data_mode.as_deref())?,
    })
    .await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&response.info)?);
    } else {
        print!("{}", response.formatted_text);
    }

    if let Some(question) = args.ask.as_deref() {
        run_ai_question(response.info, question, args.ai_model).await?;
    }

    Ok(())
}

fn parse_cluster(value: Option<&str>) -> anyhow::Result<Option<RpcCluster>> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value.to_ascii_lowercase().as_str() {
        "devnet" => Ok(Some(RpcCluster::Devnet)),
        "mainnet" | "mainnet-beta" => Ok(Some(RpcCluster::Mainnet)),
        _ => anyhow::bail!("cluster must be devnet or mainnet"),
    }
}

fn parse_data_mode(value: Option<&str>) -> anyhow::Result<Option<DebugDataMode>> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(Some(DebugDataMode::Auto)),
        "rpc_only" | "rpc-only" => Ok(Some(DebugDataMode::RpcOnly)),
        "rpc_plus_grpc" | "rpc-plus-grpc" => Ok(Some(DebugDataMode::RpcPlusGrpc)),
        _ => anyhow::bail!("data mode must be auto, rpc_only, or rpc_plus_grpc"),
    }
}

#[cfg(feature = "ai")]
async fn run_ai_question(
    info: raydium_debugger::TransactionDebugInfo,
    question: &str,
    model: Option<String>,
) -> anyhow::Result<()> {
    let response = run_ai_request(AiAskRequest {
        info,
        question: question.to_string(),
        model,
    })
    .await
    .context("AI question failed")?;
    println!("\nAI Answer ({}):\n{}", response.model, response.answer);
    Ok(())
}

#[cfg(not(feature = "ai"))]
async fn run_ai_question(
    _info: raydium_debugger::TransactionDebugInfo,
    _question: &str,
    _model: Option<String>,
) -> anyhow::Result<()> {
    anyhow::bail!(
        "AI question requested, but this binary was built without the `ai` feature. Rebuild with `cargo run --features ai -- ... --ask \"your question\"`."
    );
}
