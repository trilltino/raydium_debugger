use anyhow::{Context, Result};
use reqwest::blocking::Client;

pub(crate) fn client() -> Result<Client> {
    Client::builder()
        .user_agent("raydium-debugger-xtask")
        .build()
        .context("failed to create source HTTP client")
}

pub(crate) fn fetch(client: &Client, url: &str) -> Result<String> {
    client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::text)
        .with_context(|| format!("failed to fetch {url}"))
}
