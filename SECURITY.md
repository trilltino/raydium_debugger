# Security

## Secrets

Triton One/rpcpool URLs can contain path tokens. Keep them in `.env.local`,
operator environment variables, or server-side deployment configuration. Do not
put tokenized RPC URLs in React code, committed config, browser storage, or test
fixtures.

The app rejects browser RPC overrides. Runtime RPC selection is always resolved
server-side from `TRITON_DEVNET_RPC_URL`, `TRITON_MAINNET_RPC_URL`, and optional
Triton fallback variables.

## Local API Token

The Axum server creates a local API token at startup unless
`RAYDIUM_DEBUGGER_API_TOKEN` is set. The same-origin React app obtains it from
`/api/session` and sends it as `x-raydium-debugger-token` for mutating or local
data routes. Treat this as local drive-by request protection, not internet-grade
authentication.

## Reporting

Report suspected token leakage, RPC override bypasses, unsafe transaction
decoding behavior, or local store corruption issues to the project owner before
sharing details publicly.
