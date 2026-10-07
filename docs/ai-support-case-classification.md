# AI-assisted support case classification

The private support database holds conversation history. The pinned reference
repos contain documentation and code snapshots collected later. This workflow
combines them for **draft investigation**, while keeping historical outcome,
current general guidance, and open questions distinct. A present-day code
example never proves a past reporter's issue was fixed.

Build the pinned reference index (see `scripts/support_reference_index.py`),
then prepare evidence packets without sending anything to a model:

```powershell
py -3 scripts/support_reference_index.py build
py -3 scripts/classify-support-cases.py prepare --include-orphans
py -3 scripts/classify-support-cases.py report
```

`prepare` includes all current support cases. `--include-orphans` also creates
one packet per latest-import message not assigned to a current case; those
single messages cannot establish a historical thread resolution. Use
`--case-id` or `--limit 10` for a small batch. Packets include all member
messages, OCR records matched to the message attachment path and archived file
hash (labeled as uncertain), dated upgrade context with chronology, and up to
eight pinned repo excerpts with commit, path, and line provenance. Outputs stay in the
ignored `.raydium-debugger/ai-case-reviews/` directory.

The authenticated Codex CLI adapter is implemented and can run a small sample or a resumable archive pass:

```powershell
py -3 scripts/classify-support-cases.py classify --model-command "py -3 scripts/codex-support-model.py" --limit 10
py -3 scripts/classify-support-batch.py --cases-only
py -3 scripts/run-support-ai-until-complete.py
py -3 scripts/classify-support-cases.py report
```

The unattended worker classifies grouped cases first and then ungrouped messages, retries invalid outputs one case at a time, and waits after a model quota error. It writes private `worker-status.json` and `worker.log`. The Codex adapter uses an ephemeral session, read-only sandbox, and a structured output schema. The single-packet `--model-command` interface remains available for another chosen model.
The adapter resolves the current Codex executable on each call, including the
VS Code extension installation when `codex` is absent from `PATH`; set
`SUPPORT_CASE_CODEX_EXECUTABLE` to an explicit executable if needed. This lets
the worker resume after an extension update moves the CLI.

The adapter reads one JSON object on standard input containing `system`,
`evidence_fingerprint`, and `packet`; it prints one JSON object on standard
output. Its response must contain `outcome` (`historical_resolution`,
`general_guidance`, or `open`), `tier` (`reporter_confirmed`, `team_fixed`,
`proposed_only`, or `unknown`), `record_type` (`technical_problem`,
`information_request`, `announcement`, `chatter`, or `other`), `category`,
`diagnosis`, `resolution`,
`general_guidance`, `confidence`, `message_evidence`, `reference_ids`, and
`upgrade_ids`, and `unanswered_questions`. Message evidence uses member revision IDs and exact
body or verified attachment-text quotes; attachment quotes are flagged for visual checking. Reference IDs must come from the packet. Historical resolutions
require direct member message evidence, and ungrouped orphans cannot claim one.
The command writes `drafts.jsonl` and reuses an existing valid draft if the
packet fingerprint matches; small batch runs preserve unrelated drafts.
Classification saves an atomic checkpoint after every 25 packets (adjust with
`--checkpoint-every`) in single-packet mode; the batch worker saves after every batch.
Reprepare after imports, regrouping, or source
changes; `report` marks stale drafts. The adapter command receives private
support text, so choose its destination deliberately.

No classifier output approves a case or enters the runtime knowledge file.
Review the cited messages and code, then use the builder's evidence review,
annotation, and approval commands for guidance intended for publication.
