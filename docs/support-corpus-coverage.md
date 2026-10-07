# Support archive coverage and upgrade knowledge

The Telegram export still exists at `C:\Users\isich\Downloads\Telegram Desktop\ChatExport_2026-09-28`. A full private snapshot is at `.raydium-debugger/ChatExport_2026-09-28`, with its source path, file count, and 28 HTML SHA-256 hashes recorded in `.raydium-debugger/archive-snapshot.json`. All 28 HTML hashes match the files already imported into `.raydium-debugger/support-knowledge.sqlite`; reimport is unnecessary. The private snapshot and generated reports are ignored by Git.

The imported corpus has 25,993 support messages across 27 support HTML pages and 18 developer update messages in `messages_updates.html`. The support case builder groups 12,663 messages into 4,338 current cases. The remaining 13,330 are individual messages outside those cases. The gap audit covers both groups, including messages with no text and attachments. Its question/problem flags are triage hints, not diagnoses or resolutions.

## Rebuild the private inventories

Run these from the repository root after an import, case regrouping, or source change:

```powershell
cargo run -p xtask -- support-knowledge candidates reconcile .raydium-debugger/support-knowledge.sqlite
py -3 -X utf8 scripts/support-gap-audit.py .raydium-debugger/support-knowledge.sqlite
py -3 -X utf8 scripts/support-attachment-audit.py .raydium-debugger/support-knowledge.sqlite .raydium-debugger/ChatExport_2026-09-28
py -3 -X utf8 scripts/build-upgrade-ledger.py
py -3 -X utf8 scripts/link-support-references.py
```

To interpret screenshots locally, create an ignored Python environment and run the resumable OCR pass:

```powershell
py -3 -m venv .raydium-debugger/ocr-venv
.raydium-debugger/ocr-venv/Scripts/python.exe -m pip install rapidocr onnxruntime
.raydium-debugger/ocr-venv/Scripts/python.exe -m pip install pymupdf
.raydium-debugger/ocr-venv/Scripts/python.exe scripts/interpret-support-media.py --workers 4
py -3 -X utf8 scripts/media-evidence-report.py
```

The OCR pass records the attachment hash, message revision ID, detected lines, and confidence in private `media-ocr.jsonl`; it can resume after interruption. It also samples up to eight frames from each MP4/MOV clip and renders up to ten pages from PDFs without selectable text, recording frame times and page numbers. The joined `media-review-queue.json` identifies the current case or orphan message for each item. OCR text is only a reading aid: screenshots and videos may contain charts, motion, audio, layout, or text the model missed, and codes or addresses can be transcribed incorrectly. Inspect the original media for any published guidance. The OCR models run locally after installation.

The JSON queue and companion `media-review-queue.tsv` put media-only messages with detected problem words or no OCR result first. This is a review priority, not a diagnosis. A screenshot that says “error” might show the reporter's problem, a previous attempt, or unrelated context.

The completed local pass produced 2,908 media records: text from 2,892 of 2,903 images, all three scanned PDFs, and sampled frames from both videos. Eleven images had no OCR text; their originals were visually inspected in the private `no-ocr-visual-review.md`. They include loading spinners, a QR code, and artwork rather than an unread error screenshot. The review queue still marks them for checking against their conversations. Four oversized documents remain outside automatic text extraction.

For visual content OCR cannot explain, write a private UTF-8 note file and record it with `py -3 -X utf8 scripts/record-media-note.py <revision-id> <relative-image-path> <note-file> <reviewer>`. The command checks that the image belongs to the current inventory and stores its hash; the review queue includes the note as unverified interpretation. A changed image invalidates the note when the queue is rebuilt.

The reports live under `.raydium-debugger/gap-audit/` and `.raydium-debugger/attachment-audit/`. They include stable message revision IDs so a reviewer can return to the exact source. Rebuild them after the database changes. The reference linker proposes only topical leads and labels planned, delayed, live, and unverified material separately. It does not record an accepted answer.

The present audit flags 2,680 cases and 2,761 individual messages for possible follow-up. Only five of those targets have high-confidence topical leads in the dated upgrade ledger. Older issues and other products need their own evidence; a low match count is safer than assigning an unrelated upgrade as a fix.

The attachment inventory currently covers 2,948 attachment references. It extracts bounded text from safe text, PDF, spreadsheet, DOCX, PPTX, and HTML files where possible. HAR files yield request routes and status codes without headers, cookies, query strings, or bodies; ZIP files yield a bounded file list without unpacking. The separate OCR pass reads visible text in images and sampled video frames. Non-text visual content, audio, and unreadable files still require review. Media interpretations remain private and do not enter the runtime until a reviewer curates an incident from them.

## Dated upgrade context

`scripts/build-upgrade-ledger.py` combines all 18 imported developer update messages with 17 pinned official changelog pages from `refernce/raydium-docs-v1`. The current ledger has 39 entries because announcements linking several changes are split by document. Each entry keeps its Telegram message ID or official document URL, date, status, and pinned repository commit/path. Official changelog pages without an explicit deployment statement are `unknown`; a scheduled date is not proof of deployment. The ledger is written to `.raydium-debugger/knowledge/updates.generated.json`.

With the `ai` feature enabled, the debugger loads that file from `RAYDIUM_DEBUGGER_UPDATES_PATH` (defaulting to the generated path), retrieves a few relevant dated entries, and gives the model their source IDs for `[update:<id>]` citations. The AI answer also retains the existing `[incident:<id>]` path for separately approved, sanitized support guidance. Upgrade notices provide public release context; they cannot establish the cause of a specific transaction by themselves.

## Review boundary

### Review in the debugger

Open **Knowledge review** in the local web or desktop app. The queue covers all
17,668 prepared packets, including 4,338 grouped cases and 13,330 ungrouped
messages. Search source text, filter by record type or review state, and open a
packet to inspect the original conversation, AI draft, local rule signals,
attachment OCR, verified original images/PDFs/MP4 clips, pinned code excerpts, and
dated updates. The list places unreviewed likely resolutions first. The queue
grows as the background classifier writes more drafts.

The reviewer can mark a packet valuable, uncertain, or not useful; correct its
category, diagnosis, and outcome; add a resolution or current guidance; cite
source revision IDs and pinned references; and save the classification. Every
decision stores the reviewer, rationale, evidence fingerprint, and an audit
event in the private builder database. Changed source messages or case
membership block publication until the packet is rebuilt and checked again.

Two publication actions keep different claims separate:

- **Approve historical fix** accepts only a grouped case with a reporter
  confirmation or an explicit team fix statement, cited member messages, and
  sanitized summary and resolution. The existing builder verification runs
  before approval.
- **Publish current guidance** accepts a valuable, reviewed problem with cited
  source message(s), a sanitized summary and advice. It works for an ungrouped
  message too, but the output is labeled current guidance and does not claim
  that the historical reporter's issue was fixed.

Publication recompiles `.raydium-debugger/knowledge/incidents.generated.json`
atomically. The artifact has separate `incidents` and `guidance` arrays and
contains no private message bodies, sender names, message revision IDs, or
attachment text. The debugger's AI retrieves matching approved incidents as
`[incident:<id>]` and matching reviewed current advice as `[guidance:<id>]`.
It reads the file for each AI request, so the next question can use a newly
published entry without restarting the app. Saved private classifications and
AI drafts do not enter the prompt. Rejection or subsequent edits remove the
entry on recompilation.

### AI assisted archive triage

The optional classifier prepares a private evidence packet for each current case and, with `--include-orphans`, each ungrouped support message. A packet contains the case's own messages, matching attachment OCR and extracted document text, topical excerpts from the pinned repositories, and dated upgrade context. The repository excerpts are research leads: they can support current technical advice, but cannot prove what happened in an older conversation. A planned upgrade is never treated as a deployed fix.

```powershell
py -3 -X utf8 scripts/support_reference_index.py build
py -3 -X utf8 scripts/classify-support-cases.py prepare --include-orphans
py -3 -X utf8 scripts/classify-support-cases.py report
```

`prepare` works without a model and does not publish anything. The authenticated Codex CLI can classify packets in batches:

```powershell
py -3 -X utf8 scripts/classify-support-batch.py --cases-only
py -3 -X utf8 scripts/classify-support-batch.py
py -3 -X utf8 scripts/classify-support-cases.py report
```

The first command prioritizes grouped cases; the second resumes and includes ungrouped messages. Each completed batch is saved before the next model call. The adapter runs Codex ephemerally in a private temporary directory with a read-only sandbox and a JSON output schema. It sends private support text and attachment OCR to the authenticated model provider. The classifier validates that quoted evidence belongs to the case and that cited docs/code snippets came from that packet. Drafts and errors stay under the ignored `.raydium-debugger/ai-case-reviews/` directory. Rebuilding packets after source or repository changes changes their fingerprints, so `report` identifies stale drafts.

For an unattended pass, run `py -3 -X utf8 scripts/run-support-ai-until-complete.py`. It resumes grouped cases, then ungrouped messages, then retries validation failures one at a time. It writes progress to `.raydium-debugger/ai-case-reviews/worker-status.json` and `worker.log`, and waits before retrying after a model quota error. Put a file named `stop-worker` in that private directory to request a stop. Persistent validation failures remain review items; the worker does not publish or approve them.

For another model provider, `py -3 -X utf8 scripts/classify-support-cases.py classify --model-command "<executable>"` accepts an executable that reads one JSON request from standard input and writes one JSON answer to standard output. Use `--limit 4` or `--case-id <case-id>` to inspect a small batch first.

The classifier distinguishes a recorded historical resolution, technical guidance from docs/code, and no supported answer. It can suggest an answer for a thread that ended without confirmation, but that answer must be presented as guidance rather than as the historical fix. AI drafts do not satisfy the reviewer record required for compilation. A person still checks any case before its sanitized summary and resolution enter the runtime incident knowledge file.

The 2026-10-04 local preparation produced 17,668 packets: 4,338 cases and 13,330 ungrouped messages. Of these, 7,032 have a topical pinned repository excerpt and 67 have dated upgrade context after conservative matching. Codex classification is running with saved checkpoints; the report shows the current completed, errored, stale, and unclassified counts.

Reconciliation currently suggests 18 reporter-confirmed, 244 team-fixed, 452 proposed, and 3,624 unknown cases. Those are local rule outputs. No support case has been approved yet, so the compiled incident knowledge is still empty. Review the full conversation and cited revisions before accepting a resolution; use [resolution-reconciliation.md](resolution-reconciliation.md) for the review and compile commands. A reference lead or extracted attachment text can help investigation but does not substitute for that review.
