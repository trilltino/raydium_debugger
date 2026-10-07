# raydium_debugger

Deterministic Solana transaction diagnostics with Raydium-aware instruction decoding, execution tracing, failure classification, and integrator casebooks.

`raydium_debugger` takes an existing Solana transaction signature and turns raw RPC evidence into a structured diagnosis:

```text
transaction signature
        ↓
Triton / Solana RPC evidence
        ↓
accounts + instructions + inner instructions + logs
        ↓
execution tree + semantic decoding
        ↓
Raydium product / operation context
        ↓
failure + compute + token/account evidence
        ↓
diagnosis + recommended action
```

The project is designed for developer support and integration debugging. It prefers observed transaction evidence and deterministic protocol semantics over speculative explanations.

It does **not** create, sign, simulate, or submit transactions.

## What it does

The debugger currently provides:

- Solana transaction fetching through configured Triton One / rpcpool endpoints.
- Static and Address Lookup Table account resolution.
- Outer and inner instruction normalization.
- Program labeling across Solana and Raydium programs.
- Raydium-aware semantic instruction decoding.
- Raydium product and operation classification.
- LaunchLab-aware transaction diagnostics.
- Nested execution-tree reconstruction from logs and inner-instruction metadata.
- CPI failure identification.
- SPL Token and Token-2022 instruction evidence.
- Transaction error and Raydium error decoding.
- Account ownership, signer, writable, balance, and rent evidence.
- Token movement and Raydium account-role evidence.
- Compute-budget decoding.
- Per-invocation compute evidence where runtime logs provide it.
- Transaction-size and loaded-account-data evidence.
- Structured root-cause and recommended-action output.
- Explicit handling of signatures not observed by the selected provider.
- Local integrator and casebook storage for retaining useful transaction examples.
- CLI, Axum API, React UI, and Tauri application surfaces.
- Optional AI Q&A behind a feature flag; deterministic debugging does not require AI.
- A local Knowledge review screen for the private support archive, with separate
  approval of historical fixes and current guidance. Approved, sanitized entries
  are retrieved by AI with incident or guidance citations.

## Diagnostic philosophy

The debugger follows a simple evidence hierarchy:

```text
1. What did the transaction actually contain?
2. What did the runtime actually execute?
3. Which instruction or CPI actually failed?
4. What can Raydium protocol semantics prove?
5. What remediation follows from that evidence?
6. What remains unknown?
```

A diagnosis should distinguish between:

- **observed facts** — RPC transaction metadata, balances, instructions, logs and runtime errors;
- **decoded semantics** — known instruction layouts, account roles, arguments and error registries;
- **inference** — conclusions supported by the available evidence;
- **hypotheses** — plausible explanations that cannot be proven from the signature alone.

When the evidence is insufficient, the debugger should return `unknown` or `not proven` rather than manufacture a causal story.

## Raydium support

The debugger recognizes Raydium transaction context including:

- CPMM
- CLMM
- AMM v4
- LaunchLab

Raydium instruction and error knowledge is kept separate from generic Solana transaction decoding.

Generated registries provide reproducible protocol metadata and can be validated offline before release.

For LaunchLab, the debugger can identify relevant LaunchLab program activity and classify known operation paths from instruction and log evidence.

Current LaunchLab program:

```text
LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj
```

## Execution model

For landed transactions, the debugger builds a `TransactionDebugInfo` containing the transaction evidence and derived diagnostics.

Important structured evidence includes:

```text
TransactionDebugInfo
├── status
├── metadata
├── outer_instructions
├── decoded_instructions
├── execution_tree
├── accounts
├── account_changes
├── logs
├── program_context
├── compute_budget
├── compute_attribution
├── resource_usage
├── raydium_product
├── raydium_context
├── failure
├── root_cause
└── recommended_actions
```

Normalized instructions include both outer and inner instructions.

Where RPC metadata provides it, inner instructions retain `stack_height` so execution-tree association can use explicit runtime evidence rather than relying only on program identity.

The execution tree records parent/child invocation relationships, log ranges, decoded instruction identity, failure state, token instruction evidence, and compute evidence where available.

## Diagnosis API

The canonical diagnosis surface is `DiagnosticResponse`:

```text
DiagnosticResponse
├── observation
├── diagnosis
├── transaction: Option<TransactionDebugInfo>
└── formatted_text
```

This allows the API to represent both landed transactions and signatures that were not observed by the selected provider.

A non-observed signature is therefore a diagnostic result rather than an application error.

The diagnosis contains a human-facing title, explanation, primary action, evidence, confidence/category information, and copyable Markdown.

### HTTP endpoints

The local Axum server exposes:

```text
POST /api/diagnose    canonical diagnosis API
POST /api/debug       compatibility debug API
POST /api/ask         optional AI Q&A

GET  /api/providers
GET  /api/health
GET  /api/session
```

It also exposes local integrator/casebook routes used by the web application.

`/api/diagnose` should be preferred by new clients.

## Integrator casebooks

The web application includes a local SQLite-backed casebook system for retaining useful integration and support transactions.

A saved signature can carry context such as:

```text
integrator
casebook
signature
cluster
label
reason
outcome
Raydium product
failure category
failure code
tags
notes
pinned state
```

This makes it possible to retain real debugging examples as a regression and support knowledge base without changing the deterministic transaction-analysis path.

The Telegram support export, private coverage reports, and dated upgrade ledger are described in [Support archive coverage and upgrade knowledge](docs/support-corpus-coverage.md).

The default database is:

```text
.raydium-debugger/casebooks.sqlite
```

The location can be overridden with:

```text
RAYDIUM_DEBUGGER_CASEBOOK_PATH
```

## Setup

### Requirements

The repository currently targets Windows development first.

Core tooling:

```text
Rust 1.96.1
Node.js 22
npm
just
```

Rust is pinned by `rust-toolchain.toml`.

### Configure Triton

Copy the example environment:

```powershell
Copy-Item .env.example .env.local
```

Configure server-side Triton endpoints:

```powershell
$env:TRITON_DEFAULT_CLUSTER="devnet"
$env:TRITON_DEVNET_RPC_URL="https://<your-devnet-endpoint>/<x-token>"
$env:TRITON_MAINNET_RPC_URL="https://<your-mainnet-endpoint>/<x-token>"
```

Optional fallback endpoints:

```powershell
$env:TRITON_DEVNET_FALLBACK_RPC_URL="..."
$env:TRITON_MAINNET_FALLBACK_RPC_URL="..."
```

Optional server configuration:

```powershell
$env:RAYDIUM_DEBUGGER_API_TOKEN="..."
$env:RAYDIUM_DEBUGGER_MAX_CONCURRENT_DEBUGS="4"
```

Never expose tokenized Triton URLs in frontend code.

RPC credentials belong only in server, CLI, or trusted operator environments.

Provider metadata returned to the frontend is redacted.

## Run the web debugger

Start the development environment:

```powershell
just dev
```

Then open:

```text
http://127.0.0.1:5173
```

Vite provides React hot reload while the Rust server restarts when backend/configuration code changes.

To build the React frontend and serve it directly through Axum:

```powershell
just serve-built
```

The Axum server binds to loopback by default and refuses non-loopback binding unless explicitly overridden.

## CLI

Debug an existing signature:

```powershell
cargo run --bin raydium-debugger -- `
  --signature <TRANSACTION_SIGNATURE> `
  --cluster devnet
```

JSON output:

```powershell
cargo run --bin raydium-debugger -- `
  --signature <TRANSACTION_SIGNATURE> `
  --cluster devnet `
  --json
```

Disable RPC fallback:

```powershell
cargo run --bin raydium-debugger -- `
  --signature <TRANSACTION_SIGNATURE> `
  --cluster devnet `
  --no-fallback
```

Require v1 transaction reads:

```powershell
$env:RAYDIUM_DEBUGGER_TX_V1_READ_REQUIRED="1"
```

## Raydium registries

Protocol error and instruction metadata are maintained through the `xtask` workspace crate.

### Error registry

Validate the committed snapshot offline:

```powershell
cargo run -p xtask -- raydium-registry validate
```

Refresh from upstream sources:

```powershell
cargo run -p xtask -- raydium-registry generate
```

Check the committed snapshot against upstream:

```powershell
cargo run -p xtask -- raydium-registry drift
```

### Instruction registry

Validate the committed instruction snapshot:

```powershell
cargo run -p xtask -- raydium-instructions validate
```

Regenerate it from upstream Raydium IDLs:

```powershell
cargo run -p xtask -- raydium-instructions generate
```

Check for upstream drift:

```powershell
cargo run -p xtask -- raydium-instructions drift
```

Normal CI uses offline validation. Network-dependent drift checks are intended for explicit refresh/drift workflows rather than deterministic builds.

## Development checks

Run the normal development gate:

```powershell
just check
```

Run the release/audit gate:

```powershell
just release-check
```

Run browser E2E tests:

```powershell
just e2e
```

The browser suite covers desktop Chrome, Android touch, a narrow 320px phone,
and iPhone Safari via WebKit. Install its browser binaries after frontend setup:

```powershell
cd web
npx playwright install chromium webkit
npm run test:e2e -- --project=mobile --project=mobile-small --project=mobile-safari
```

Mobile checks include touch navigation, Help filters, and preserving transaction
evidence when moving between Help and Debug. WebKit device emulation does not
replace testing on a physical iPhone.

Run opt-in live Triton checks:

```powershell
just live-check
```

The CI/release gates cover Rust formatting, workspace checks, Clippy with warnings denied, Rust tests, documentation, dependency policy, Raydium registry validation, frontend typechecking/building, npm audit, and Playwright browser tests.

The Windows native suite launches the built desktop application with WebView2
and verifies actual IPC, restart recovery and interrupted-work retry:

```powershell
npm --prefix web run build
cargo build -p raydium-debugger-tauri --features custom-protocol -p xtask --locked
cargo install tauri-driver --version 2.0.6 --locked
./scripts/install-edge-driver.ps1
npm --prefix web run test:native
```

Verified delivery results, private review-pack instructions and the performance
comparison are recorded in [docs/delivery-verification.md](docs/delivery-verification.md).

## Security model

Protected data includes:

- tokenized Triton RPC URLs;
- the local API token;
- saved integrator signatures and casebook context;
- transaction debug output;
- optional AI-provider inputs.

Important trust boundaries include CLI arguments, `.env.local`, RPC responses, Axum requests, local SQLite persistence, Tauri IPC, and optional AI-provider calls.

The web application cannot supply arbitrary RPC URLs to the server. Debug requests use the configured Triton endpoint for the selected cluster.

Secret-bearing RPC URLs are redacted before provider metadata is exposed.

The default server is intended for local use and binds to loopback.

## AI

AI support is optional.

The default build excludes it:

```powershell
cargo build
```

Enable all features, including AI support:

```powershell
cargo build --all-features
```

The deterministic debugger does not depend on an AI provider.

AI should consume structured diagnostic evidence; it is not the source of truth for transaction execution, protocol decoding, or failure classification.

## Scope

`raydium_debugger` is an observability and diagnostic tool.

It does:

```text
fetch
decode
classify
trace
explain
store debugging cases
```

It does not:

```text
construct transactions
sign transactions
submit transactions
trade
manage wallets
custody keys
```

## Audit notes

- Rust toolchain: `1.96.1`
- Rust edition: `2021`
- License: `AGPL-3.0-or-later`
- Windows-first local development
- CI: `windows-latest`
- Default build excludes optional AI support
- External/user-controlled inputs should return structured errors rather than panic
- Process-startup invariants and tests may use panic-style assertions
- Direct dependency advisories should be resolved before release
- Tracked transitive advisories are documented in `deny.toml`

## References

- Raydium documentation: https://docs.raydium.io/
- Raydium GitHub: https://github.com/raydium-io
- Raydium SDK V2: https://github.com/raydium-io/raydium-sdk-V2
- Triton One documentation: https://docs.triton.one/getting-started

## Status

This project is under active development.

The current focus is making Raydium transaction diagnosis increasingly precise, reproducible, and useful for real developer/integrator support while keeping the core analysis deterministic and evidence-driven.

## References

- Triton One getting started: https://docs.triton.one/getting-started
- Raydium docs: https://docs.raydium.io/
- Raydium LaunchLab SDK constants: https://docs.rs/raydium-launchlab-sdk/latest/src/raydium_launchlab/constants.rs.html

Live collection, recent-group investigation, desktop evidence paths, and matching evaluation are documented in [docs/investigation-runtime.md](docs/investigation-runtime.md).
