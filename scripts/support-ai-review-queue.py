"""Write a private evidence pack for reviewing AI support-case drafts."""

from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]
PRIVATE = ROOT / ".raydium-debugger/ai-case-reviews"
sys.path.insert(0, str(Path(__file__).resolve().parent))
SPEC = importlib.util.spec_from_file_location(
    "classify_support_cases", Path(__file__).with_name("classify-support-cases.py"))
classification = importlib.util.module_from_spec(SPEC)
assert SPEC and SPEC.loader
SPEC.loader.exec_module(classification)


def load_drafts(path: Path) -> dict[str, dict]:
    if not path.is_file():
        return {}
    with path.open(encoding="utf-8") as stream:
        return {item["case_id"]: item for line in stream if line.strip()
                for item in [json.loads(line)]}


def rank(draft: dict) -> tuple:
    outcome = draft.get("outcome")
    tier = draft.get("tier")
    confidence = draft.get("confidence")
    has_action = bool(draft.get("resolution") and any(
        word in draft["resolution"].lower() for word in
        ("retry", "refresh", "reinstall", "change", "update", "cancel", "switch", "use ", "set ")))
    return (0 if outcome == "historical_resolution" else 1,
            0 if tier == "reporter_confirmed" else 1,
            0 if has_action else 1,
            0 if confidence == "high" else 1,
            draft["case_id"])


def build(packets_path: Path, drafts_path: Path, limit: int) -> str:
    drafts = load_drafts(drafts_path)
    selected = sorted((draft for draft in drafts.values()
                       if draft.get("status") == "ai_draft_unreviewed"
                       and draft.get("classification_version") == classification.CLASSIFICATION_VERSION
                       and draft.get("kind") == "case"), key=rank)[:limit]
    ids = {draft["case_id"] for draft in selected}
    packets = {}
    with packets_path.open(encoding="utf-8") as stream:
        for line in stream:
            packet = json.loads(line)
            if packet["case_id"] in ids:
                packets[packet["case_id"]] = packet
    lines = ["# Private AI support-case review queue", "",
             "AI drafts are leads. Check the full source thread and attachments before accepting a historical resolution.", ""]
    for number, draft in enumerate(selected, 1):
        packet = packets.get(draft["case_id"])
        if not packet or packet["evidence_fingerprint"] != draft["evidence_fingerprint"]:
            continue
        lines.extend([
            f"## {number}. {draft['case_id']}", "",
            f"- Outcome: `{draft['outcome']}` / `{draft['tier']}`; confidence `{draft['confidence']}`",
            f"- Type: `{draft['record_type']}`; category: {draft['category']}",
            f"- Diagnosis: {draft.get('diagnosis') or 'Unknown'}",
            f"- Historical resolution: {draft.get('resolution') or 'None stated'}",
            f"- Current guidance: {draft.get('general_guidance') or 'None stated'}",
            f"- Unanswered: {'; '.join(draft.get('unanswered_questions', [])) or 'None listed'}",
            "", "Evidence quotes:", "",
        ])
        for item in draft.get("message_evidence", []):
            source = " (attachment text; inspect original)" if item["revision_id"] in draft.get(
                "attachment_quote_revision_ids", []) else ""
            lines.append(f"- Revision `{item['revision_id']}`{source}: {item['quote']}")
        refs = {ref["id"]: ref for ref in packet["references"]}
        if draft.get("reference_ids"):
            lines.extend(["", "Pinned references:", ""])
            for identifier in draft["reference_ids"]:
                ref = refs.get(identifier)
                if ref:
                    lines.append(f"- `{identifier}` {ref['authority']} `{ref['repo']}` "
                                 f"`{ref['path']}:{ref['start_line']}` at `{ref['commit']}`")
        lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--packets", type=Path, default=PRIVATE / "packets.jsonl")
    parser.add_argument("--drafts", type=Path, default=PRIVATE / "drafts.jsonl")
    parser.add_argument("--output", type=Path, default=PRIVATE / "review-queue.md")
    parser.add_argument("--limit", type=int, default=50)
    args = parser.parse_args()
    if args.limit < 1:
        parser.error("limit must be positive")
    output = build(args.packets, args.drafts, args.limit)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(output, encoding="utf-8")
    print(f"Wrote private review queue: {args.output}")


if __name__ == "__main__":
    main()
