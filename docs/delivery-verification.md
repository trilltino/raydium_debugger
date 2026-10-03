# Delivery verification — 3 October 2026

Verification runs on Windows with Rust 1.96.1, Node 22.18.0 and Playwright 1.63.0. Raw execution logs are stored
locally under `target/verification/`, with browser and native logs under `target/`.
Private archive text, databases and reviewer packages stay in ignored directories.

## Verified checks

| Check | Result | Local evidence |
| --- | --- | --- |
| Workspace tests, all features, locked, live RPC enabled | 81 passed, zero failed; all three live transaction fixtures checked | `target/verification/rust-tests-final.log` |
| Rust formatting | Passed after live-test setup fix | `target/verification/format-final.log` |
| Workspace check, all targets, locked | Passed | `target/verification/rust-check.log` |
| Clippy, all targets and features, locked, warnings denied | Passed | `target/verification/clippy-final.log` |
| Rustdoc, all features, no dependencies, locked, warnings denied | Passed | `target/verification/rustdoc-final.log` |
| Windows desktop build with embedded frontend | Passed | `target/verification/native-build.log` |
| Actual Windows native suite | All five tests passed | `target/native-suite.log` |
| Complete offline browser suite | 61 passed; 16 live opt-ins and three desktop executions of mobile-only tests skipped | `target/browser-suite.log` |
| Complete browser suite, live RPC enabled | 77 passed, zero failed; three desktop executions of mobile-only tests skipped | `target/browser-suite-live.log` |
| Frontend typecheck and production build | Passed | `target/verification/frontend-build.log` |
| Production npm audit | Zero vulnerabilities | `target/verification/frontend-audit.log` |
| Runtime dependency boundaries | All three runtime crates passed | `target/verification/runtime-boundaries.log` |
| Rust dependency policy | Advisories, bans, licenses and sources passed | `target/verification/dependencies.log` |
| Raydium error and instruction registries | Both passed | `target/verification/registry.log`, `target/verification/instruction-registry.log` |
| Private review pack generation and provenance | 30 families, 60 queries; 19 development and 11 held-out families | `target/verification/review-pack-generation.log`, `target/verification/review-pack.log` |

Native verification launches the built Tauri application with WebView2 and
uses actual IPC and the shared Rust service. Only its RPC network boundary is
replaced by a local fixture. It covers symptom-only UI entry, signature-only
and combined evidence, recent groups and event replay, provider failure,
completed-result recovery after restart, and forced process interruption with
explicit retry. The harness uses classic WebDriver, writes driver diagnostics
to each isolated run directory, and supplies the required Solana RPC metadata
`status` field.

Browser verification covers desktop Chromium, Pixel 5, a 320px phone and
iPhone Safari emulation in WebKit. Live checks verify combined investigations,
successful devnet transactions, unknown mainnet custom errors and Token-2022
insufficient funds. Completed investigations are checked through the rendered
UI, persisted browser cursor, authenticated event replay and result lookup.
This avoids relying on Chromium retaining a cancelled SSE response body.
Browser test discovery excludes the native Mocha specs. Physical iPhone
testing remains outside the emulation coverage.

Dependency policy retains the existing `RUSTSEC-2025-0141` exemption in
`deny.toml`. The policy run also reports an upstream missing-license-field
warning for `solana-config-interface 2.0.1` and a yanked `yoke-derive 0.8.3`
warning. Passing the configured policy does not mean there are no exemptions
or upstream warnings.

## Private domain review

Generate a fresh package without overwriting a reviewer's work:

```powershell
cargo run -p xtask -- support-knowledge review-pack .raydium-debugger/review-pack-2026-10-03 .raydium-debugger/support-knowledge.sqlite
py -3 scripts/verify-review-pack.py .raydium-debugger/review-pack-2026-10-03/review-pack.json .raydium-debugger/support-knowledge.sqlite
```

The package contains 30 conversation families and 60 verbatim reporter queries.
Sampling tags suggest review priorities; they are not approved classifications.
Each family belongs to one split, including its two queries. Source messages,
sender identities and source-file references are private review context.

The domain reviewer must explicitly set each case's `approval` and `reviewer`,
and each query's `relevant`, `forbidden` and `reviewer`. Empty result lists are
valid labels for abstention; null means unfinished review. Review context may
contain the answer, so do not add responder resolution text to query features.
Curate and approve only cases supported by evidence, then compile a sanitized
incident artifact before running:

```powershell
cargo run -p xtask -- support-knowledge evaluate-review-pack .raydium-debugger/review-pack-2026-10-03/review-pack.json <sanitized-artifact.json>
```

The evaluator refuses incomplete human labels and evaluates development and
held-out partitions separately. Generating and verifying the package does not
establish retrieval quality on the support archive.

## Performance reproduction

```powershell
cargo build --release -p xtask --locked
py -3 scripts/ingestion-performance.py --archive 'C:\Users\isich\Downloads\Telegram Desktop\ChatExport_2026-09-28' --label post-refactor --runs 3 --baseline target/performance/baseline/report.json
```

The benchmark freezes the executable, uses fresh databases for each run,
measures import and unchanged import against the same archive, evaluates the
16-query synthetic matching fixture and collects 100 local RPC observations.
Reported values are medians of three process wall-clock measurements and
sampled Windows peak working-set measurements. Matching p50/p95 measures query
work inside the process separately from startup overhead. Positive percentage
changes mean more time or memory. This small sample is descriptive, not a
statistical significance claim. Matching labels remain synthetic and the
collector uses a local fixture rather than provider latency.

The recorded baseline has 28 source files and 26,011 message revisions in each
run. Comparison refuses different imported row counts. The original baseline
does not record source hashes, so equal counts alone cannot prove byte-for-byte
archive identity. New reports record executable and baseline report hashes.

## Measured performance

All three post-refactor runs preserved 28 source files and 26,011 message
revisions. Each collector run stored exactly 100 observations. The measurements
ran after the browser suites and Cargo builds finished.

| Operation | Recorded baseline median | Post-refactor median | Time change | Peak working-set change |
| --- | ---: | ---: | ---: | ---: |
| Archive import | 7.436 s | 2.545 s | -65.8% | -0.3% |
| Unchanged import | 2.006 s | 0.729 s | -63.7% | +0.8% |
| Matching process, including startup | 37.860 ms | 15.273 ms | -59.7% | -14.7% |
| Local RPC collector | 0.743 s | 0.287 s | -61.4% | -1.6% |

The median of the per-run matching p50 values changed from 98 to 36 microseconds;
the median p95 changed from 262 to 112 microseconds. The synthetic fixture still
has precision@3, recall@3 and reciprocal rank of 1.0, eight correct abstentions
and zero contradiction violations. All three runs passed its engineering gate.
Archive domain labels remain unreviewed.

Timing fell in every measured operation. Import memory stayed approximately
flat, including a small increase for unchanged imports. The original baseline
does not record host load or cache state; the measured differences alone cannot
attribute all speed gains to the refactor.

Sanitized raw measurements are retained in
[verification/performance-baseline.json](verification/performance-baseline.json)
and [verification/performance-post-refactor.json](verification/performance-post-refactor.json).
These reports contain counts and synthetic metrics, with no archive messages
or provider credentials. Local frozen executables and benchmark databases stay
under ignored `target/performance/`.
