set shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# List available tasks.
default:
    just --list

# Run the local dev app with Vite HMR and Rust server restart-on-change.
dev:
    npm --prefix web install
    powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File scripts/dev.ps1

# Run only the Vite dev server; keep `just server` running in another terminal.
web-dev:
    npm --prefix web run dev

# Serve API and the latest built web/dist assets.
server:
    cargo run -p raydium-debugger-server

# Build the React app and serve the full real E2E app through Axum.
serve-built:
    npm --prefix web install
    npm --prefix web run build
    cargo run -p raydium-debugger-server

# Build the frontend bundle that Axum serves.
web-build:
    npm --prefix web run build

# Run the core Rust checks.
check:
    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features --locked
    cargo run -p xtask -- raydium-registry check
    npm --prefix web run typecheck
    npm --prefix web run build

# Run release/audit gates that are useful before tagging or sharing builds.
release-check:
    cargo doc --workspace --no-deps --locked
    cargo deny check
    cargo package --list --allow-dirty
    cargo run -p xtask -- raydium-registry check
    npm --prefix web audit --omit=dev
    npm --prefix web run test:e2e

# Run live Triton/Raydium regression tests when Triton env is configured.
live-check:
    $env:RUN_LIVE_E2E="1"; cargo test --workspace --all-features --locked live_
    $env:RUN_LIVE_E2E="1"; npm --prefix web run test:e2e

# Run the default non-live browser smoke tests against Axum.
e2e:
    npm --prefix web run test:e2e

# Run dependency advisory checks when local tools are available.
audit:
    npm --prefix web audit --omit=dev
    if (Get-Command cargo-deny -ErrorAction SilentlyContinue) { cargo deny check } else { Write-Host "cargo-deny not installed; skipping Rust dependency policy check" }
