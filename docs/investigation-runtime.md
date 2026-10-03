# Investigation runtime

The HTTP and Tauri shells share the `raydium-investigation` crate. Requests support a signature, symptom, both, or `recent_fingerprint` plus an explicit cluster. Fingerprints select operational observations; they never fabricate a transaction signature. The public groups endpoint and investigation results contain redacted metadata rather than raw logs.

## Live RPC collection

Set `TRITON_DEVNET_RPC_URL` or `TRITON_MAINNET_RPC_URL` in the collector process environment. Unlike the server and desktop launcher, xtask does not automatically load `.env.local`. Use the existing configured provider; no endpoint is accepted from public investigation requests.

```text
cargo run -p xtask -- support-knowledge observations collect-rpc <address> --cluster devnet --since <unix-seconds> --poll-seconds 15
```

`<address>` is an account/program to observe. Collection uses finalized [getSignaturesForAddress pagination](https://solana.com/docs/rpc/http/getsignaturesforaddress) followed by [getTransaction](https://solana.com/docs/rpc/http/gettransaction). It collects operational logs without signing or sending transactions. Custom errors are attributed to the first failed program log; instruction names remain unscoped raw log text unless independently decoded.

Defaults: `.raydium-debugger/observations.sqlite`, 25 signatures per page, four pages per pass, 30-second request timeout, maximum 4 MiB response, 30-day retention. Override `--database`, `--page-size` (1â€“100), `--max-pages` (1â€“100). Omitting `--poll-seconds` runs one bounded pass. Continuous polling retries errors with backoff capped at 60 seconds. Stop with Ctrl+C.

Keep the same endpoint, address, cluster and `--since` timestamp when restarting. These identify the durable collector. Live polling clamps its scan floor to the rolling retention window, so the original collector can restart after a long outage. A page commits observations and its cursor together; failures retry the entire page. A partially drained backlog retains the old head, pending new head and backward cursor until drained, so a page budget cannot silently skip intermediate signatures. Two collectors cannot overwrite the same cursor with stale state. Replays deduplicate by source signature and cluster. Missing transactions, timestamps or logs stop progress and remain an explicit provider availability issue.

For a bounded backfill:

```text
cargo run -p xtask -- support-knowledge observations collect-rpc <address> --cluster devnet --since <start-unix-seconds> --end <end-unix-seconds> --max-pages 4
```

Repeat the exact command until `range complete: true`, or add `--poll-seconds 15` to continue automatically. Start/end must lie within the retention window. A completed backfill stays complete on restart. RPC history limits may prevent completing a range; absence of observations is not proof of absence of an event.

## Desktop and web entry points

In the app, select the cluster and click **Browse recent observations**, then **Investigate** on a group. This works without a signature. The authenticated HTTP endpoint is `GET /api/observations/groups?cluster=devnet`; submit its fingerprint to `POST /api/investigate`. Completed web results remain recoverable through `GET /api/investigations/<id>`.

Desktop uses its application data directory for `investigations.sqlite`, `observations.sqlite`, and `incidents.generated.json`. To use the same collector and reviewed artifact as the server, set absolute paths before starting Tauri:

```text
RAYDIUM_DEBUGGER_OBSERVATIONS_DATABASE_PATH=<absolute collector database path>
RAYDIUM_DEBUGGER_KNOWLEDGE_PATH=<absolute compiled incident registry path>
RAYDIUM_DEBUGGER_INVESTIGATION_PATH=<absolute private ledger path>
```

Missing optional stores leave deterministic diagnosis available. No approved incidents are invented for an empty registry. Native progress uses a Tauri channel; symptom-only, signature and recent-group investigations all call the shared service. The separate Windows native suite launches the built desktop window and tests actual IPC, restart recovery and interruption. The browser command-bridge regression covers frontend routing separately.

## Match evaluation

```text
cargo run -p xtask -- support-knowledge evaluate-matches tests/fixtures/incident-match-benchmark.json
cargo run -p xtask -- support-knowledge evaluate-matches <reviewed-benchmark.json> --require-reviewed
```

The report includes precision among returned top-three results, recall@3, reciprocal rank@3, previous matcher recall@3, correct abstentions, forbidden-result violations and p50/p95 latency. The engineering quality gate requires all expectations to pass, zero forbidden results and recall improvement over the frozen baseline. `--require-reviewed` additionally requires `review_status: "reviewed"` and a nonempty `reviewer`. Change these only after actual domain review; the supplied 16-query synthetic suite is explicitly `pending_domain_review`. Passing this fixture does not establish quality on the unreviewed support archive.

Optional registry predicates use `{ "feature": "program", "value": "<program-id>" }` under `must`, `should`, and `must_not`. Supported facts are cluster, program, product, instruction, error and outcome. Error predicates require a program predicate, and matching checks the actual error's program attribution separately from the transaction's invoked programs. Program IDs preserve Base58 case. Unknown requirements weaken a match; known contradictions reject it. Optional `retired`, `valid_from`, and `valid_until` fields control eligibility. Unspecified constraints remain unknown; text relevance is never transaction proof.

## Verification

```text
cargo test -p raydium-debugger-server -p xtask
npm --prefix web run test:e2e
```

For successful real RPC evidence verification, set `RUN_LIVE_E2E=1`, configure the devnet provider (environment or `.env.local`), and run:

```text
npm --prefix web run test:e2e -- --project=desktop --grep "investigates a signature and symptom"
```

The live test uses the devnet success signature from `tests/live_signatures.toml` and checks a non-null landed transaction, positive slot, successful outcome, evidence ledger and persisted result equality. Provider errors fail this test. Offline browser runs use isolated stores under `target/` and deliberately unavailable RPC endpoints, and do not read the private archive. Historical signature retention can require refreshing the live fixture.

Verified delivery results for 3 October 2026, including native Windows automation, Rust gates, private review-pack provenance and the performance comparison, are recorded in [delivery-verification.md](delivery-verification.md). Domain-reviewed query labels and additional live API adapters remain open.
