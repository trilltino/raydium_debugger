# raydium_debugger

Standalone Solana transaction debugger with Raydium program labeling and
LaunchLab-aware diagnostics.

This was extracted from `XFChess/backend/src/signing/solana/debug.rs` and the
minimal transaction fetch support it needed. It fetches a confirmed transaction,
decodes static and loaded account evidence, prints logs/CPI frames, classifies
likely root cause, identifies Raydium products/phases where possible, and emits
recommended next actions.

## Usage

For the web app/server path, configure Triton One/rpcpool on the server side:

```powershell
Copy-Item .env.example .env.local
$env:TRITON_DEFAULT_CLUSTER="devnet"
$env:TRITON_DEVNET_RPC_URL="https://xfsoluti-solanad-d155.devnet.rpcpool.com/<x-token>"
$env:TRITON_MAINNET_RPC_URL="https://xfsoluti-solanam-739d.mainnet.rpcpool.com/<x-token>"
```

Then run the local app with hot reload:

```powershell
just dev
```

Open `http://127.0.0.1:5173`. React changes hot reload through Vite, and Rust
server/config changes restart the local Axum API automatically.

To serve the built React bundle directly from Axum instead:

```powershell
just serve-built
```

The React app only receives redacted provider metadata. Triton `x-token` values
must stay in server, CLI, or operator environments.

Then debug a signature:

```powershell
cargo run --bin raydium-debugger -- `
  --signature <TRANSACTION_SIGNATURE> `
  --cluster devnet
```

JSON output:

```powershell
cargo run --bin raydium-debugger -- --signature <SIG> --json
```

Disable fallback:

```powershell
cargo run --bin raydium-debugger -- --signature <SIG> --no-fallback
```

Useful environment variables:

- `TRITON_DEVNET_RPC_URL`: required Triton/rpcpool devnet endpoint.
- `TRITON_MAINNET_RPC_URL`: required Triton/rpcpool mainnet endpoint.
- `TRITON_DEVNET_FALLBACK_RPC_URL`: optional Triton/rpcpool devnet fallback endpoint.
- `TRITON_MAINNET_FALLBACK_RPC_URL`: optional Triton/rpcpool mainnet fallback endpoint.
- `RAYDIUM_DEBUGGER_TX_V1_READ_REQUIRED=1`: fail hard if v1 transaction reads are not supported by the RPC.

Never put a tokenized Triton URL in frontend code. Triton secret tokens belong
in backend, CLI, or operator environments only. This CLI redacts rpcpool token
paths and query API keys in output metadata.

## Raydium LaunchLab

LaunchLab transactions are detected through the current Raydium LaunchLab
program ID:

```text
LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj
```

The debugger identifies the Raydium product and, for LaunchLab, infers whether
the signature looks like initialize, buy, sell, graduate, or an unknown
LaunchLab path based on instruction/log evidence. It only debugs existing
signatures; it does not create, sign, simulate, or send transactions.

## Checks

```powershell
just check
just release-check
```

## Audit Support

- Toolchain: Rust `1.96.1`, edition 2021, pinned by `rust-toolchain.toml`.
- Supported local target: Windows development first; CI runs the same gate on
  `windows-latest`.
- Feature matrix: default build excludes AI; `--all-features` includes optional
  `genai` AI Q&A.
- Panic policy: user, file, network, RPC, and browser/API inputs should return
  structured errors. Panics are acceptable only for process startup invariants or
  tests.
- Protected assets: Triton tokenized RPC URLs, local API token, saved integrator
  signature library, and transaction debug outputs.
- Trust boundaries: CLI args, `.env.local`, Triton RPC responses, Axum JSON
  requests, local JSON store, Tauri IPC, and optional AI provider calls.
- Release gates: formatting, locked Rust check/test/doc, clippy, cargo-deny,
  npm audit, frontend typecheck/build, and Playwright smoke tests.

Known advisory notes:

- Some Solana/Tauri transitive dependencies currently report unmaintained
  advisories. They are tracked in `deny.toml` with comments. Direct advisories
  in this crate must be fixed before release.
- `genai` remains optional behind the `ai` feature; normal deterministic
  debugging does not call AI providers.

## References

- Triton One getting started: https://docs.triton.one/getting-started
- Raydium docs: https://docs.raydium.io/
- Raydium LaunchLab SDK constants: https://docs.rs/raydium-launchlab-sdk/latest/src/raydium_launchlab/constants.rs.html
