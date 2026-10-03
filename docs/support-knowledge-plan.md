# CoralOS-free debugger and support knowledge plan

Updated 2 October 2026. Proposed end-state; private archive import, candidate review, Tantivy message search, deterministic investigation, and restartable RPC collection are implemented. Domain-reviewed match evaluation and broader production hardening remain open. This adapts the 52-page Raydium_Debugger.pdf architecture without CoralOS: keep its deterministic evidence contract, recent-log intelligence, investigation API, evidence ledger, progress stream and production hardening. Move support-corpus import and review ahead of the application workflow, following this newer plan. There is no multi-agent runtime requirement.

## Outcome and evidence contract

Import the Telegram support archive into traceable records, assemble candidate cases, enrich signatures through the existing Rust debugger, review cases, and publish reusable diagnostic knowledge. A signature, symptom, or both should return useful matches with reasons, contradictions, missing evidence, historical dates and recommended checks.

Preserve the PDF's evidence precedence: current transaction/accounts, recent execution observations, current API/RPC observations, reviewed historical cases, user report, then AI hypotheses. Retrieval similarity and model predictions are candidate-selection signals, not proof of a cause. Unknown required facts lower match strength; known contradictions reject an incident. The deterministic debugger must work with every optional subsystem disabled.

## Crate allocation

| Crate | Proposed allocation | Introduction gate |
|---|---|---|
| rusqlite | Private source/review store; transactions, constraints and migrations | First implementation; server already pins 0.37.0 with bundled SQLite |
| tantivy | Rebuildable lexical indexes of messages and case summaries | First searchable corpus |
| petgraph | In-memory typed graph for candidate conversations; persist edges in SQLite | Candidate extraction |
| fastembed | Local case embeddings and optional cross-encoder reranking | Reviewed lexical benchmark exists |
| hnsw_rs | Approximate vector retrieval, keyed to stable case IDs | Exact vector-search baseline shows a measured latency/memory need |
| polars | Optional offline feature snapshots and aggregate analytics | SQL reports become insufficient |
| linfa + selected algorithm crates | Offline clustering and later supervised domain/relevance experiments | Enough reviewed labels and a leakage-resistant evaluation set |
| smartcore | Alternative offline model experiment, such as random forest/boosting | A documented baseline limitation merits comparison; not a second default ML stack |

Use scraper for DOM parsing, SHA-256 for source identity and bs58 for entity validation. Exact new versions/features are selected after Windows/Rust/workspace compilation and dependency-policy checks. Do not install every crate into the core decoder.

Sources: [rusqlite](https://docs.rs/rusqlite/latest/rusqlite/), [Tantivy](https://docs.rs/tantivy/latest/tantivy/), [Petgraph](https://docs.rs/petgraph/latest/petgraph/graph/index.html), [FastEmbed](https://docs.rs/fastembed/latest/fastembed/), [HNSW](https://docs.rs/hnsw_rs/latest/hnsw_rs/hnsw/index.html), [Polars lazy API](https://docs.rs/polars/latest/polars/docs/lazy/index.html), [Linfa](https://docs.rs/linfa/latest/linfa/), [SmartCore](https://docs.rs/smartcore/latest/smartcore/).

## Workspace boundaries

- `crates/raydium-knowledge`: shared incident/feature types, registry loader, symptom normalization and deterministic constraint matcher. Core decoding calls this through `src/knowledge_bridge.rs` without depending on raw corpus storage or ML.
- `crates/raydium-knowledge-builder`: private import/store/graph/enrichment/review/compiler implementation. Existing `xtask support-knowledge ...` delegates here; this avoids making the command dispatcher a large importer.
- `crates/raydium-knowledge-search`: lexical retrieval and optional semantic adapters. Public deployment reads sanitized approved-case indexes; operator deployment may explicitly enable a separate private corpus index.
- `raydium-debugger-server` owns the first investigation endpoint, evidence ledger and progress stream. Keep orchestration as a small bounded Rust service in the existing server; extract a crate only if a second caller needs it.
- A later `raydium-observability` crate can own recent-log ingestion, normalization, fingerprints and queries. It must not change the deterministic transaction decoder.
- Analytics and model training remain optional offline builder commands/features. Polars, Linfa and SmartCore do not belong in the default API/Tauri dependency path.
- No `raydium-coral` crate or CoralOS session layer is planned. Optional model summarization runs behind the server boundary and receives only evidence selected by the investigation service.

## End-to-end runtime without CoralOS

The same Rust server owns the investigation lifecycle; there is no external agent coordinator.

1. The UI submits a signature, a symptom, or both to `/api/investigate`; the server validates input and creates a durable investigation ID.
2. The deterministic debugger fetches and analyzes a transaction when a signature is present. A symptom-only request does not invent a transaction failure.
3. The server queries bounded recent-log observations when available, then retrieves reviewed incidents using exact fields and Tantivy. Optional semantic candidates may be added later.
4. Each returned fact or case is recorded in a private evidence ledger with provenance and a stable evidence ID. Unknowns and contradictions remain explicit.
5. The server streams progress over SSE and returns a deterministic result even when search, recent logs, or optional model services are unavailable.
6. Only after this flow works, an optional single-model synthesis step may explain the evidence. It cannot call arbitrary tools, read raw support data, or assert evidence without valid IDs; the server rejects invalid references.
7. The UI renders observed transaction facts, recent observations, historical matches, missing evidence and recommended checks separately.

Entry modes are signature only, symptom only, signature plus symptom, and a recent-log cluster. AI output is explanatory, never the source of transaction or historical facts.

## Phase 1 — Reliable import and searchable records

Implemented slice: `xtask support-knowledge import-html` imports numbered Telegram message pages and the updates page into a versioned SQLite database under `.raydium-debugger/`; `stats` reports per-corpus counts and `validate` checks SQLite integrity, foreign keys and source-message identity. Import source hashes make unchanged reruns a no-op and changed files retain a new revision. `resolve-replies` and every import resolve reply IDs against the latest source-file revisions within each corpus, preserving unresolved and ambiguous links explicitly. Schema v3 stores deterministic entity candidates: Base58 tokens decoding to 32-byte addresses or 64-byte signatures, and numeric `Custom(n)`/`0x…` error forms; these are not chain-verified or assigned to a program. `entity-search` resolves exact candidate values, including equivalent decimal/hex error forms. `index` builds a derived Tantivy index of the newest revision per source file; `search` refreshes a stale index, supports literal multi-term queries and support/announcement filters, and prints bounded snippets. The initial Tantivy index covers message body/sender and stored source metadata. Remaining Phase 1 work includes product/instruction/symptom extraction, exact entity fields in Tantivy, reviewed cases and broader malformed-input reporting.

Import the 27 numbered support HTML pages as one corpus, naturally ordered, and retain `messages_updates.html` as a distinct announcement source type. Resolve replies across pages. Preserve missing/invalid timestamps, joined-message author inheritance provenance, edited messages, media references and unresolvable replies without inventing content. Media ingestion means recording references initially, not running OCR or executing files.

SQLite schema: corpora, source_files, import_runs, message_revisions, entities, reply_edges, cases, case_messages, case_edges, transaction_snapshots, review_events, incidents, case_incidents and artifact_versions. Use corpus-scoped message identity; a filename or file hash alone must not define a message across changing exports. Source-file hashes identify imported versions, and every derived record points to its source revision and extractor version. Duplicate/edited exports must not duplicate messages or silently erase prior reviewer work.

Store canonical entities separately: decoded 64-byte signature candidates, decoded 32-byte address candidates, explorer cluster hints, product names, instruction names, program-scoped numeric error codes, symptoms and outcomes. Shape validity is not RPC confirmation. Ambiguous addresses retain unknown role until context/account evidence resolves it. Normalize Unicode for search while retaining original text. Error numbers alone must never imply program identity.

Tantivy indexes narrative fields for lexical relevance and uses exact fields for addresses, signatures, product, program, instruction and error keys. Keep raw text search local. Normalize `Custom(38)` and `0x26` into comparable numeric signals, with program scope when available. User-entered search text is treated literally by default, with controlled structured filters.

Indexes are derived artifacts. Track DB revision, indexing watermark and schema version; interrupted index writes can be resumed or rebuilt. Publish index generations atomically. Exact entity lookup remains available through SQLite if text search is unavailable.

Exit criteria: a second import changes no counts; cross-page replies resolve; malformed messages are counted/reported; source revisions remain traceable; exact signature/code queries find known fixtures; index rebuild returns equivalent results.

## Phase 2 — Candidate cases and debugger evidence

Implemented candidate workflow: Petgraph groups only current support messages joined by resolved support reply edges; announcement reply chains, isolated messages and shared addresses/programs do not merge. Candidate IDs derive from sorted corpus/file/message identities. Manual merge/split assignments use stable source-message identities, survive automatic rebuilds, and append reviewer rationale to an audit log. `candidates list`, `show`, `review`, `annotate`, `merge`, `split`, and bounded `enrich` are available. Enrichment reuses the existing debugger service, capped at five signatures per run and cached by signature/cluster. Approving requires curated summary/resolution text. The compiler emits only approved annotations, not raw support messages. The supplied archive currently forms 4,338 reply-connected support candidate groups.

Build a typed Petgraph graph from persisted edges: replies, adjacent joined messages, shared signatures, specific addresses/tickets and contextual time/product links. Distinguish hard conversation links from weak contextual links. Common program IDs, common mints, popular pool addresses and time adjacency must not collapse the corpus into giant connected components. Shared entities can link separate cases without merging them; constrain case windows and record edge reasons. Use connected components only over accepted conversation edges, then support reviewer merge/split overrides.

Cases hold reported problem, proposed resolution, product, domain, symptom tags and supporting message IDs. Separate user reports, support suggestions, explicit resolution messages and verified execution facts. Suggested case summaries can be assisted by templates/AI, but their state remains unreviewed.

Enrichment reuses the existing diagnostic service for extracted signatures, with a small per-run limit, signature/cluster caching and timestamped SQLite snapshots. Missing historical RPC data is a recorded observation, not a failed transaction. Store transaction slot/time, instruction/error evidence, parser/registry version and account-observation time. Current account state is not silently presented as state at the historical transaction slot.

Review commands: candidates list/show, annotate, enrich, merge/split, approve and reject. Decisions are append-only with reviewer/time/rationale. Approved annotations compile to `knowledge/incidents.generated.json`; the artifact contains curated text and aggregate evidence counts, never source messages, senders, signatures or addresses. The current private corpus has no approved annotations, so its generated incident registry is intentionally empty.

Exit criteria: reviewers can reconstruct every candidate; unrelated messages sharing a program ID stay separate; duplicate imports preserve decisions; success and missing-pool symptoms do not become fabricated on-chain failures.

## Phase 3 — Reviewed knowledge and hybrid retrieval

Compile sorted, sanitized `knowledge/incidents.generated.json` plus approved-case search documents. Strip support identities, private links and unnecessary wallet/media details. Raw/review stores and private search artifacts stay under `.raydium-debugger/`, separate from casebooks. Record corpus, schema, review, source and compiler revisions in artifact manifests.

Each incident has must/should/must-not predicates, program/product/version scope, validity dates, historical/active/retired status, recommendations and safe case summaries. Treat release announcements as a separate version ledger; a planned date or changelog URL is not deployment confirmation.

Runtime path:

1. Build features from the signature and/or normalized symptom.
2. Reject known contradictions and preserve unknown requirements.
3. Union exact structured matches, Tantivy candidates and optional semantic candidates.
4. Fuse lexical/vector ranks (initially reciprocal-rank fusion), retaining exact program/error matches independently of fuzzy top-k limits.
5. Recheck eligibility; optionally rerank the eligible shortlist.
6. Return evidence-backed strong/moderate/weak matches, missing signals and historical caveats. Similarity score is not diagnostic confidence.

FastEmbed runs embeddings and supported rerankers locally; its docs describe initial model downloads and synchronous inference. Keep model execution on a bounded worker, cache artifacts deliberately, and pin model identity/revision, tokenizer, dimension and normalization. Embed reviewed case summaries and problem/resolution/evidence chunks; keep exact signatures/addresses in structured fields. Start with exact vector search for a correctness baseline. Add HNSW only after measuring its recall/latency tradeoff. Build indexes per artifact/model generation, retain ID mappings and exclude withdrawn cases before and after retrieval.

Use the model's prescribed query/document formatting consistently. If embedding models/indexes are unavailable or stale, return deterministic/lexical results. No model download should occur unexpectedly during a diagnostic request.

Exit criteria: paraphrases find appropriate reviewed cases; exact matches are preserved; contradictory cases never win through embedding similarity; removed cases disappear; lexical/deterministic operation survives model/index failure.

## Phase 4 — Analytics, evaluation and learning

Implemented regression baseline: `evaluate-matches` compares the previous all-term matcher against weighted problem-term coverage, explicit must/should/must-not facts, outcome/product/date scope, custom-error program attribution, negation and token boundaries. Results expose reasons, missing signals and rejected contradictions. Sixteen synthetic labeled queries currently pass with recall@3 1.0 versus baseline 0.625, eight correct abstentions and zero forbidden matches. These are engineering regression expectations, not reviewed archive labels; `--require-reviewed` refuses release promotion until named domain review is recorded. Human-reviewed queries, a held-out corpus and broad relevance validation remain necessary before calling matches dependable.

Start statistics in SQL. Export versioned case-level feature snapshots for optional Polars aggregation: program × error × product × token standard × symptom, missing evidence, review yield and changes over time. Raw chat volume is not independent labeled training data.

Create reviewer-labeled queries and relevant/irrelevant case pairs. Split by conversation, duplicated-case family and time/program era; keep future messages and resolution text out of features for historical prediction tasks. Freeze training/test snapshots. Compare exact/rule baseline, lexical-only, hybrid retrieval, and reranking using precision/recall@k, ranking quality, abstention, contradiction violations and p50/p95 latency. Add a release gate requiring zero contradiction violations in the fixture suite and demonstrated relevance improvement before promoting optional models.

Linfa clustering can suggest duplicate cases/themes for review. Later supervised models can suggest failure domains, relevance or next useful checks when labels justify them. Use specific Linfa algorithm crates rather than expecting the base crate to contain every implementation. A SmartCore experiment is an alternative if richer supervised models demonstrate an advantage. Neither clustering nor classification creates approved facts or changes deterministic diagnosis. A learned model may abstain; human review remains the path to production rules.

## Phase 5 — Recent-log observability

Implemented offline ingestion interface: `observations ingest-json` accepts bounded JSON/NDJSON observations from `rpc`, `raydium_api`, `indexer`, or `partner_api`; validates clusters/entity shapes, fingerprints normalized logs without address/signature values, deduplicates by source identity, and prunes a 30-day window. `observations search` returns local matches. Agent/runtime telemetry sources are rejected. `observations collect-rpc` now polls finalized signatures and transaction logs from an explicitly selected configured cluster, with a bounded page budget, durable head/backfill cursors, atomic observation/checkpoint commits, retry backoff and restart recovery. `--end` bounds historical backfill within the 30-day retention window; `--poll-seconds` enables continued polling. Missing transaction/log evidence holds the page for retry rather than advancing past it. API collectors and websocket subscriptions remain optional future adapters. Do not treat missing observations as proof that an event did not happen.

Exit criteria: ingestion resumes after restart, replays without duplicates, backfills a requested range, and returns explainable matching clusters with source/time provenance. The pipeline can be disabled without changing transaction diagnosis.

## Phase 6 — Deterministic investigation API and UI

Implemented initial web investigation flow: `/api/investigate` accepts signature-only, symptom-only, and combined requests; it persists the run/evidence ledger, streams progress and the final result over SSE, and exposes completed results by ID for recovery. The React app has a separate symptom/signature investigation form while preserving the existing Debug workflow. Results combine the deterministic transaction diagnosis, matches from the sanitized approved registry, and redacted recent-observation metadata. The public API never reads or returns raw support messages/logs. The authenticated recent-observation groups endpoint and UI can start a fingerprint-scoped investigation without inventing a signature. Tauri now uses the same Rust investigation service, validation, registry matching, observation lookup and durable ledger as HTTP, including symptom-only requests. Automatic progress replay after stream reconnection remains future work.

Exit criteria currently met for signature-only, symptom-only and combined investigations without an LLM; successful transactions are not mislabeled as program failures; returned evidence has stable IDs; interrupted streams can recover completed results by ID; server restart does not corrupt the durable ledger; raw support messages remain private. Recent-group entry is implemented. Replaying progress after reconnect remains future work. A successful devnet browser-to-real-RPC combined investigation was verified on 2 October 2026 (slot 504513907), including transaction evidence and durable result lookup; its opt-in Playwright test fails if the provider fetch fails or returns no transaction.

## Phase 7 — Optional single-model synthesis

Add one bounded synthesis call only if evaluations show that deterministic results need a natural-language explanation. It receives the investigation's curated evidence, not direct RPC, filesystem, database or unrestricted network access. Require structured output with evidence IDs, unknowns and suggested checks; validate every ID, render factual summaries with their cited evidence, and label hypotheses separately. Model text never becomes a persisted transaction fact or approved incident. Apply request timeouts, token/cost budgets, cancellation and a no-model fallback. Do not introduce specialist-agent graphs or a CoralOS dependency.

Exit criteria: model-off and provider-failure paths return useful deterministic results; invalid evidence references are rejected; synthesis cannot alter persisted transaction facts or approved incidents; latency and cost are measured against the deterministic baseline.

## Phase 8 — Production hardening

Add request/rate limits, tenant and private-corpus isolation, secret handling, retention controls, bounded RPC/model work and operational metrics. Test collector restart/backfill, index recovery, investigation recovery, degraded modes and current-behavior review for historical incidents. Keep agent/model telemetry, if synthesis exists, separate from Raydium evidence.

## First implementation slice

Recommended end-to-end order:

1. Protect current deterministic debugger behavior with regression fixtures and confirm the core works with optional features disabled.
2. Implement SQLite migrations/import identity and import the archive; make repeated imports idempotent and revisions traceable.
3. Add entity extraction, reply resolution, Petgraph candidate grouping, debugger enrichment and reviewer merge/split/approval commands.
4. Compile sanitized reviewed incidents, add Tantivy lexical search, and benchmark exact/lexical matching. Seed the PDF's suggested incidents only after checking the archive evidence and current applicability.
5. Add bounded recent-log collection, fingerprints, replay deduplication and backfill.
6. Ship the no-model `/api/investigate`, private evidence ledger, SSE progress and UI for signature/symptom workflows.
7. Add FastEmbed and reranking only if the reviewed lexical benchmark demonstrates a retrieval gap. Begin with exact vector search; add `hnsw_rs` only after measured scale needs it.
8. Add Polars reports only when SQL is insufficient. Trial Linfa clustering or supervised models only after reviewed labels, leakage-resistant splits and a held-out evaluation set exist. Treat SmartCore as an alternative experiment, not a second default ML stack.
9. Consider optional single-model synthesis after deterministic end-to-end behavior is useful; keep provider failure and model-off behavior fully functional.
10. Harden privacy, limits, recovery and monitoring; measure quality, latency and cost before expanding scope.

Implemented commands:

```text
cargo run -p xtask -- support-knowledge import-html <export-directory>
cargo run -p xtask -- support-knowledge stats
cargo run -p xtask -- support-knowledge validate
cargo run -p xtask -- support-knowledge resolve-replies
cargo run -p xtask -- support-knowledge extract-entities
cargo run -p xtask -- support-knowledge entity-search 'Custom(38)' --corpus support
cargo run -p xtask -- support-knowledge index
cargo run -p xtask -- support-knowledge search "pool not showing" --corpus support --limit 10
cargo run -p xtask -- support-knowledge candidates rebuild
cargo run -p xtask -- support-knowledge candidates list 30
cargo run -p xtask -- support-knowledge candidates show <case-id>
cargo run -p xtask -- support-knowledge candidates annotate <case-id> <product> <domain> <summary> <resolution>
cargo run -p xtask -- support-knowledge candidates review <case-id> approve "reason"
cargo run -p xtask -- support-knowledge candidates merge <case-id> <case-id> "reason"
cargo run -p xtask -- support-knowledge candidates split <case-id> <message-id,...> "reason"
cargo run -p xtask -- support-knowledge candidates signatures 30
cargo run -p xtask -- support-knowledge candidates enrich <case-id> --cluster mainnet --limit 1
cargo run -p xtask -- support-knowledge compile
cargo run -p xtask -- support-knowledge observations ingest-json <file.json|file.ndjson>
cargo run -p xtask -- support-knowledge observations search "Custom(38)" --cluster mainnet
cargo run -p xtask -- support-knowledge observations collect-rpc <address> --cluster devnet --since <unix-seconds> --poll-seconds 15
cargo run -p xtask -- support-knowledge evaluate-matches tests/fixtures/incident-match-benchmark.json
cargo run -p raydium-debugger-server -- --bind 127.0.0.1:8787
```

The web API exposes `POST /api/investigate` as SSE and `GET /api/investigations/<id>` for completed-result recovery. The web UI uses both; Tauri calls the shared investigation service through its command bridge. `GET /api/observations/groups?cluster=devnet` exposes redacted recent groups for direct investigation entry.

Collector configuration, backfill/restart procedures, desktop paths and benchmark review gates are documented in [investigation-runtime.md](investigation-runtime.md).

The refactor's verified checks, private 30-conversation/60-query review package
and recorded-baseline performance comparison are documented in
[delivery-verification.md](delivery-verification.md). Human approval and
relevance labels remain pending.

Tests include import/revision identity, joined authors, cross-page/missing/ambiguous replies, hub-address graph isolation, Unicode, scoped errors, RPC-not-observed snapshots, reviewer merge/split persistence, deterministic compilation, sanitization, index-generation recovery, temporal match validity and offline fallback. Recent-log tests cover replay deduplication, 30-day pruning, normalized fingerprints and provenance. End-to-end tests cover signature-only, symptom-only and combined investigations, evidence IDs, SSE completion and lookup recovery, related cases, model-off/provider-failure behavior and privacy boundaries without exposing raw support data.
