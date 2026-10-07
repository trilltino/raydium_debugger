"""Suggest dated public references for private support gaps; never infer a fix.

The audit identifies revision IDs only. Message text is read from the private
SQLite database and is never copied into the output. All links need review.
"""

from __future__ import annotations

import argparse
from collections import Counter
from datetime import datetime
import json
from pathlib import Path
import re
import sqlite3


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_AUDIT = ROOT / ".raydium-debugger/gap-audit/support-gaps.json"
DEFAULT_LEDGER = ROOT / ".raydium-debugger/knowledge/updates.generated.json"
DEFAULT_DATABASE = ROOT / ".raydium-debugger/support-knowledge.sqlite"
DEFAULT_OUTPUT = ROOT / ".raydium-debugger/gap-audit/reference-candidates.json"
STOP = set("""about above after also been before change changes create current does error errors
failed failing from have into issue main more need only pool pools program programs raydium
should some support swap token tokens update updated upgrade using with would account accounts
please there their this that these those what when which where your will work working
transaction transactions instruction instructions version users user data docs new old
same now can not are and for the you use how why any all has was its get
https http com github master rather side than anything open out already call created existing
one but allow check normally state updates creation fees fee mint config platform scale
now live will may soon also through here make made still related our they them what
""".split())
PRODUCTS = ("clmm", "cpmm", "launchlab", "ammv4", "stable-amm", "token2022")
MIN_OVERLAP = 2
MAX_LINKS = 3
DISTINCTIVE = set("""anchor customizable creator dynamic freeze frozen lamports limit
openbook order orders permissioned position positions single-sided token2022 transfer
observation nonce tick keeper withheld withdraw withdrawpnl liquidity nft""".split())


def terms(value: str) -> set[str]:
    value = re.sub(r"([a-z])([A-Z])", r"\1 \2", value)
    value = value.lower().replace("token-2022", "token2022")
    value = re.sub(r"\bamm\s*v4\b", "ammv4", value)
    value = re.sub(r"\bstable\s+(?:swap\s+)?amm\b", "stable-amm", value)
    return {word for word in re.findall(r"[a-z][a-z0-9-]{2,}", value)
            if word not in STOP and not word.isdigit()}


def product_terms(words: set[str]) -> set[str]:
    return words.intersection(PRODUCTS)


def parse_date(value: str | None):
    if not value:
        return None
    try:
        return datetime.strptime(value[:10], "%d.%m.%Y").date()
    except ValueError:
        try:
            return datetime.strptime(value[:10], "%Y-%m-%d").date()
        except ValueError:
            return None


def relation(status: str, reference_date, message_date) -> str:
    if status == "live" and message_date and reference_date and reference_date <= message_date:
        return "live_reference_check"
    if status == "live":
        return "later_live_context"
    if status == "planned":
        return "planned_context"
    if status == "delayed":
        return "delayed_context"
    return "unverified_context"


def rank(text: str, message_date, entries: list[dict]) -> list[dict]:
    query = terms(text)
    query_products = product_terms(query)
    if not query_products:
        return []
    candidates = []
    for entry in entries:
        # A multi-change announcement can have several ledger entries. Prefer
        # the pinned page for this entry over its shared announcement body.
        source = entry.get("reference_body") or entry.get("body") or ""
        path = entry.get("reference_path") or ""
        subject = terms(source + " " + path)
        path_terms = terms(path)
        # Shared multi-product announcements must be narrowed to the specific
        # changelog page attached to this ledger entry.
        entry_products = product_terms(path_terms) if path else product_terms(subject)
        shared_products = query_products & entry_products
        if not shared_products:
            continue
        shared_topics = sorted((query & subject) - set(PRODUCTS))
        if len(shared_topics) < MIN_OVERLAP or not set(shared_topics).intersection(DISTINCTIVE):
            continue
        reference_date = parse_date(entry.get("date"))
        if message_date and reference_date and (reference_date - message_date).days > 90:
            continue
        if path and not ((query & path_terms) - set(PRODUCTS)):
            continue
        status = entry.get("status", "unknown")
        item = {
            "source_id": entry["id"],
            "source_message_id": entry.get("source_message_id"),
            "source_url": entry.get("source_url"),
            "source_date": entry.get("date"),
            "source_status": status,
            "relationship": relation(status, reference_date, message_date),
            "reference_repo": entry.get("reference_repo"),
            "reference_commit": entry.get("reference_commit"),
            "reference_path": path or None,
            "match_reasons": {
                "shared_products": sorted(shared_products),
                "shared_terms": shared_topics[:8],
                "date_relation": (
                    "unknown" if not message_date or not reference_date else
                    "published_by_message_date" if reference_date <= message_date else
                    "published_after_message_date"
                ),
            },
        }
        score = len(shared_topics) + 3 * len(shared_products)
        if reference_date and message_date and reference_date <= message_date:
            score += 1
        if status == "live":
            score += 1
        candidates.append((score, item))
    candidates.sort(key=lambda pair: (-pair[0], pair[1]["source_id"]))
    return [item for _, item in candidates[:MAX_LINKS]]


def build(audit: dict, ledger: dict, connection: sqlite3.Connection) -> dict:
    if audit.get("report_kind") != "private_support_gap_triage":
        raise ValueError("unexpected support gap audit format")
    entries = ledger.get("entries")
    if not isinstance(entries, list):
        raise ValueError("upgrade ledger has no entries")
    targets = []
    for case in audit.get("cases", []):
        if case.get("needs_followup"):
            targets.append(("case", case["case_id"], case["message_revision_ids"]))
    for orphan in audit.get("orphan_messages", []):
        if orphan.get("needs_followup"):
            revision = orphan["message_revision_id"]
            targets.append(("orphan_message", str(revision), [revision]))
    revisions = sorted({revision for _, _, ids in targets for revision in ids})
    messages = {}
    for offset in range(0, len(revisions), 800):
        chunk = revisions[offset:offset + 800]
        placeholders = ",".join("?" for _ in chunk)
        sql = ("SELECT revision_id,body,date_title FROM message_revisions "
               f"WHERE corpus_id='support' AND revision_id IN ({placeholders})")
        messages.update({row[0]: row[1:] for row in connection.execute(sql, chunk)})
    if len(messages) != len(revisions):
        raise ValueError("audit references missing support message revisions")
    suggestions = []
    for kind, identifier, ids in targets:
        body = " ".join(messages[revision][0] for revision in ids)
        dates = [parse_date(messages[revision][1]) for revision in ids]
        dates = [date for date in dates if date]
        links = rank(body, min(dates) if dates else None, entries)
        if links:
            suggestions.append({"target_kind": kind, "target_id": identifier,
                                "message_revision_ids": ids, "reference_candidates": links})
    return {
        "report_kind": "private_reference_candidates",
        "disclaimer": "These are topical leads, not verified diagnoses or resolutions. Planned notices are never fixes.",
        "target_count": len(targets),
        "targets_with_candidates": len(suggestions),
        "suggestions": suggestions,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audit", type=Path, default=DEFAULT_AUDIT)
    parser.add_argument("--ledger", type=Path, default=DEFAULT_LEDGER)
    parser.add_argument("--database", type=Path, default=DEFAULT_DATABASE)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()
    audit = json.loads(args.audit.read_text(encoding="utf-8"))
    ledger = json.loads(args.ledger.read_text(encoding="utf-8"))
    uri = args.database.resolve().as_uri() + "?mode=ro"
    with sqlite3.connect(uri, uri=True) as connection:
        report = build(audit, ledger, connection)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{report['targets_with_candidates']} of {report['target_count']} flagged targets have reference leads: {args.output}")


if __name__ == "__main__":
    main()
