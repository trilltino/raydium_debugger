"""Resume AI triage of all private support packets using batched Codex CLI calls.

Historical fixes remain unreviewed drafts. Each batch is saved before the next
model call, so rerunning this command continues from completed classifications.
"""

from __future__ import annotations

import argparse
from collections import Counter
import importlib.util
import json
from pathlib import Path
import sqlite3
import sys


ROOT = Path(__file__).resolve().parents[1]
SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))


def load_script(name: str):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), SCRIPT_DIR / name)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


classification = load_script("classify-support-cases.py")
model_adapter = load_script("codex-support-model.py")
DEFAULT_PACKETS = ROOT / ".raydium-debugger/ai-case-reviews/packets.jsonl"
DEFAULT_DRAFTS = ROOT / ".raydium-debugger/ai-case-reviews/drafts.jsonl"
DEFAULT_DATABASE = ROOT / ".raydium-debugger/support-knowledge.sqlite"


def priority_map(database: Path) -> dict[str, int]:
    if not database.is_file():
        return {}
    uri = database.resolve().as_uri() + "?mode=ro"
    with sqlite3.connect(uri, uri=True) as connection:
        rows = connection.execute("""
            SELECT c.case_id,
                   MAX(CASE s.signal WHEN 'reporter_confirmation' THEN 3
                       WHEN 'team_fix' THEN 2 WHEN 'proposal' THEN 1 ELSE 0 END)
            FROM candidate_cases c
            LEFT JOIN case_resolution_suggestions s ON s.case_id=c.case_id
            WHERE c.is_current=1 AND c.corpus_id='support'
            GROUP BY c.case_id
        """)
        return {case_id: 3 - score for case_id, score in rows}


def packet_offsets(path: Path, priorities: dict[str, int], include_orphans: bool) -> list[tuple[int, int, str]]:
    indexed = []
    with path.open("rb") as stream:
        while True:
            offset = stream.tell()
            line = stream.readline()
            if not line:
                break
            item = json.loads(line)
            if item["kind"] == "orphan" and not include_orphans:
                continue
            priority = 4 if item["kind"] == "orphan" else priorities.get(item["case_id"], 3)
            indexed.append((priority, offset, item["case_id"]))
    indexed.sort(key=lambda row: (row[0], row[2]))
    return indexed


def compact_packet(packet: dict) -> dict:
    return {
        "case_id": packet["case_id"], "kind": packet["kind"],
        "messages": [{"revision_id": message["revision_id"],
                      "source_message_id": message["source_message_id"],
                      "sender": message["sender"], "date": message["date"],
                      "body": message["body"],
                      "attachments": [{"status": attachment["status"],
                                       "text": attachment.get("text", "")[:2000],
                                       "visual_review_needed": attachment.get("visual_review_needed", True)}
                                      for attachment in message["attachments"]]}
                     for message in packet["messages"]],
        "references": [{"id": ref["id"], "repo": ref["repo"],
                        "authority": ref["authority"], "commit": ref["commit"],
                        "path": ref["path"], "start_line": ref["start_line"],
                        "end_line": ref["end_line"], "text": ref["text"][:2500]}
                       for ref in packet["references"][:4]],
        "upgrade_context": [{"id": item["id"], "date": item["date"],
                             "status": item["status"], "chronology": item["chronology"],
                             "summary": item.get("summary"),
                             "body_excerpt": (item.get("body_excerpt") or "")[:1200]}
                            for item in packet.get("upgrade_context", [])[:3]],
    }


def valid_prior(packet: dict, draft: dict | None) -> bool:
    return bool(draft and draft.get("status") == "ai_draft_unreviewed"
                and draft.get("evidence_fingerprint") == packet["evidence_fingerprint"]
                and draft.get("classification_version") == classification.CLASSIFICATION_VERSION)


def covered_prior(packet: dict, draft: dict | None, skip_invalid: bool) -> bool:
    if valid_prior(packet, draft):
        return True
    return bool(skip_invalid and draft and draft.get("status") == "invalid_output"
                and draft.get("evidence_fingerprint") == packet["evidence_fingerprint"]
                and draft.get("classification_version") == classification.CLASSIFICATION_VERSION)


def classify_batch(packets: list[dict], model: str | None) -> list[dict]:
    answers = model_adapter.run_batch(
        classification.SYSTEM_INSTRUCTIONS, [compact_packet(p) for p in packets], model)
    by_id = {}
    for answer in answers:
        if not isinstance(answer, dict) or not isinstance(answer.get("case_id"), str):
            raise ValueError("batch contains an answer without case_id")
        identifier = answer.pop("case_id")
        if identifier in by_id:
            raise ValueError(f"duplicate batch answer: {identifier}")
        by_id[identifier] = answer
    if set(by_id) != {packet["case_id"] for packet in packets}:
        raise ValueError("batch answer IDs do not match requested packets")
    results = []
    for packet in packets:
        try:
            result = classification.validate_result(packet, by_id[packet["case_id"]])
        except ValueError as error:
            result = {"case_id": packet["case_id"], "kind": packet["kind"],
                      "evidence_fingerprint": packet["evidence_fingerprint"],
                      "classification_version": classification.CLASSIFICATION_VERSION,
                      "status": "invalid_output", "error": str(error),
                      "raw_answer": by_id[packet["case_id"]]}
        results.append(result)
    return results


def run(packets_path: Path, drafts_path: Path, database: Path, *,
        batch_size: int = 5, max_batch_chars: int = 90000, limit: int | None = None,
        include_orphans: bool = True, model: str | None = None,
        skip_invalid: bool = False) -> dict:
    if batch_size < 1 or max_batch_chars < 1000:
        raise ValueError("invalid batch bounds")
    existing = {item["case_id"]: item for item in classification.read_jsonl(drafts_path)}
    offsets = packet_offsets(packets_path, priority_map(database), include_orphans)
    counts = Counter()
    current = []
    current_chars = 0
    attempts = 0
    locally_revalidated = False

    def flush() -> None:
        nonlocal attempts, current, current_chars
        if not current:
            return
        def record(batch: list[dict], results: list[dict]) -> None:
            nonlocal attempts
            for result in results:
                existing[result["case_id"]] = result
                counts[result.get("outcome", result["status"])] += 1
            attempts += len(batch)
            classification.write_jsonl(drafts_path, existing.values())
            print(json.dumps({"processed": attempts, "stored": len(existing),
                              "last_case": batch[-1]["case_id"], "counts": dict(counts)}),
                  flush=True)
        try:
            record(current, classify_batch(current, model))
        except ValueError as error:
            # A malformed multi-case response should not halt the archive pass.
            # Retry each packet independently so identities cannot be crossed.
            if len(current) == 1:
                record(current, [{"case_id": current[0]["case_id"],
                    "kind": current[0]["kind"], "evidence_fingerprint": current[0]["evidence_fingerprint"],
                    "classification_version": classification.CLASSIFICATION_VERSION,
                    "status": "invalid_output", "error": str(error)}])
            else:
                for packet in current:
                    try:
                        answer = classify_batch([packet], model)
                    except ValueError as item_error:
                        answer = [{"case_id": packet["case_id"], "kind": packet["kind"],
                            "evidence_fingerprint": packet["evidence_fingerprint"],
                            "classification_version": classification.CLASSIFICATION_VERSION,
                            "status": "invalid_output", "error": str(item_error)}]
                    record([packet], answer)
        current = []
        current_chars = 0

    with packets_path.open("rb") as stream:
        for _, offset, identifier in offsets:
            stream.seek(offset)
            packet = json.loads(stream.readline())
            previous = existing.get(identifier)
            if (previous and previous.get("status") == "invalid_output"
                    and previous.get("raw_answer")
                    and previous.get("evidence_fingerprint") == packet["evidence_fingerprint"]
                    and previous.get("classification_version") == classification.CLASSIFICATION_VERSION):
                try:
                    existing[identifier] = classification.validate_result(
                        packet, previous["raw_answer"])
                    locally_revalidated = True
                    counts["revalidated"] += 1
                except ValueError:
                    pass
            if covered_prior(packet, existing.get(identifier), skip_invalid):
                counts["reused"] += 1
                continue
            if limit is not None and attempts + len(current) >= limit:
                break
            compact = compact_packet(packet)
            size = len(json.dumps(compact, ensure_ascii=True))
            if current and (len(current) >= batch_size or current_chars + size > max_batch_chars):
                flush()
            current.append(packet)
            current_chars += size
        flush()
    if locally_revalidated and not attempts:
        classification.write_jsonl(drafts_path, existing.values())
    return {"attempted": attempts, "stored": len(existing), "counts": dict(counts),
            "remaining": max(0, len(offsets) - sum(1 for _, _, identifier in offsets
                    if valid_prior_index(identifier, existing, skip_invalid)))}


def valid_prior_index(identifier: str, existing: dict, skip_invalid: bool) -> bool:
    # The final report performs the full fingerprint check; this count only
    # estimates outstanding cases without reading every packet twice.
    draft = existing.get(identifier)
    return bool(draft and draft.get("classification_version") == classification.CLASSIFICATION_VERSION
                and (draft.get("status") == "ai_draft_unreviewed"
                     or (skip_invalid and draft.get("status") == "invalid_output")))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--packets", type=Path, default=DEFAULT_PACKETS)
    parser.add_argument("--drafts", type=Path, default=DEFAULT_DRAFTS)
    parser.add_argument("--database", type=Path, default=DEFAULT_DATABASE)
    parser.add_argument("--batch-size", type=int, default=5)
    parser.add_argument("--max-batch-chars", type=int, default=90000)
    parser.add_argument("--limit", type=int)
    parser.add_argument("--cases-only", action="store_true")
    parser.add_argument("--skip-invalid", action="store_true")
    parser.add_argument("--model")
    args = parser.parse_args()
    try:
        summary = run(args.packets, args.drafts, args.database,
                      batch_size=args.batch_size, max_batch_chars=args.max_batch_chars,
                      limit=args.limit, include_orphans=not args.cases_only,
                      model=args.model, skip_invalid=args.skip_invalid)
        print(json.dumps(summary))
    except Exception as error:
        print(f"Batch stopped; completed drafts remain saved: {error}", file=sys.stderr)
        raise SystemExit(1)


if __name__ == "__main__":
    main()
