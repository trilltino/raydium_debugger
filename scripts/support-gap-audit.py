"""Read-only inventory of support threads that may still need an answer.

This is a triage report, not an answer generator. It never promotes an inferred
resolution to reviewed knowledge. Output belongs in the private operator area.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import re
import sqlite3
from collections import Counter
from pathlib import Path


RULE_VERSION = 3  # crates/raydium-knowledge-builder/src/resolution.rs
PROBLEM_TERMS = re.compile(
    r"\b(error|fail(?:ed|ing|ure|s)?|can't|cannot|unable|issue|problem|"
    r"stuck|broken|revert(?:ed|ing)?|incorrect|wrong|missing|help|why|how|"
    r"doesn't|not working|does not work)\b",
    re.IGNORECASE,
)
PRODUCT_TERMS = {
    "clmm": "refernce/tino_radium_read/clmm/raydium-clmm",
    "cpmm": "refernce/tino_radium_read/cpmm-cp-swap/raydium-cp-swap",
    "swap": "refernce/raydium-docs-v1",
    "amm": "refernce/tino_radium_read/amm/raydium-amm/program/src",
    "launchlab": "refernce/raydium-docs-v1",
    "token-2022": "refernce/raydium-docs-v1",
    "sdk": "refernce/tino_radium_read/sdk-api-integration/raydium-sdk-V2/src",
}


def fingerprint(messages: list[sqlite3.Row]) -> str:
    """Match the builder's source-message fingerprint, including sender and revision."""
    digest = hashlib.sha256()
    for message in messages:
        body = message["body"].encode("utf-8")
        digest.update(int(message["revision_id"]).to_bytes(8, "little", signed=True))
        digest.update(len(body).to_bytes(8, "little"))
        digest.update(body)
        digest.update((message["sender"] or "").encode("utf-8"))
    return digest.hexdigest()


def classify_case(
    case_id: str,
    status: str,
    messages: list[sqlite3.Row],
    scan: sqlite3.Row | None,
    suggestions: list[sqlite3.Row],
    review: sqlite3.Row | None,
) -> dict:
    current_hash = fingerprint(messages)
    scan_current = bool(
        scan and scan["rule_version"] == RULE_VERSION and scan["fingerprint"] == current_hash
    )
    signals = {row["signal"] for row in suggestions} if scan_current else set()
    tier = (
        "not_scanned" if not scan_current else
        "confirmed" if "reporter_confirmation" in signals else
        "team_fixed" if "team_fix" in signals else
        "proposed" if "proposal" in signals else "unknown"
    )
    review_state = (
        "unreviewed" if review is None else
        "reviewed" if review["fingerprint"] == current_hash else "stale_review"
    )
    accepted_outcome = review["outcome"] if review_state == "reviewed" else None
    # A question mark in a message or common support vocabulary is only a queueing hint.
    likely_problem = any(
        "?" in message["body"] or PROBLEM_TERMS.search(message["body"])
        for message in messages
    )
    content = " ".join(message["body"].lower() for message in messages)
    hints = [path for term, path in PRODUCT_TERMS.items() if term in content]
    return {
        "case_id": case_id,
        "case_review_status": status,
        "message_revision_ids": [message["revision_id"] for message in messages],
        "source_message_ids": [message["source_message_id"] for message in messages],
        "likely_question_or_problem": bool(likely_problem),
        "suggested_resolution_tier": tier,
        "resolution_signal_revision_ids": sorted(
            {row["revision_id"] for row in suggestions} if scan_current else set()
        ),
        "resolution_review_state": review_state,
        "accepted_outcome": accepted_outcome,
        "needs_followup": bool(
            likely_problem and accepted_outcome not in {"confirmed", "team_fixed"}
        ),
        "reference_search_hints": sorted(set(hints)),
    }


def build_report(connection: sqlite3.Connection) -> dict:
    connection.row_factory = sqlite3.Row
    cases = connection.execute(
        "SELECT case_id, review_status FROM candidate_cases "
        "WHERE is_current=1 AND corpus_id='support' ORDER BY case_id"
    ).fetchall()
    rows = connection.execute(
        "SELECT cm.case_id, m.revision_id, m.source_message_id, m.sender, m.body "
        "FROM candidate_case_messages cm "
        "JOIN candidate_cases c ON c.case_id=cm.case_id "
        "JOIN message_revisions m ON m.revision_id=cm.revision_id "
        "WHERE c.is_current=1 AND c.corpus_id='support' "
        "ORDER BY cm.case_id,m.revision_id"
    ).fetchall()
    grouped: dict[str, list[sqlite3.Row]] = {}
    for row in rows:
        grouped.setdefault(row["case_id"], []).append(row)
    scans = {
        row["case_id"]: row for row in connection.execute(
            "SELECT case_id,fingerprint,rule_version FROM case_resolution_scans"
        )
    }
    suggestions: dict[str, list[sqlite3.Row]] = {}
    for row in connection.execute(
        "SELECT case_id,revision_id,signal FROM case_resolution_suggestions"
    ):
        suggestions.setdefault(row["case_id"], []).append(row)
    reviews = {
        row["case_id"]: row for row in connection.execute(
            "SELECT case_id,outcome,fingerprint FROM case_resolution_reviews"
        )
    }
    records = [
        classify_case(
            case["case_id"], case["review_status"], grouped.get(case["case_id"], []),
            scans.get(case["case_id"]), suggestions.get(case["case_id"], []),
            reviews.get(case["case_id"]),
        )
        for case in cases
    ]
    assert len(records) == len(cases)
    assert len({record["case_id"] for record in records}) == len(cases)
    if any(not record["message_revision_ids"] for record in records):
        raise ValueError("current support case without source messages")
    current_messages = connection.execute(
        "SELECT m.revision_id,m.source_message_id,m.sender,m.body,m.media_refs_json,"
        "f.file_name FROM message_revisions m "
        "JOIN source_files f ON f.source_file_id=m.source_file_id "
        "WHERE m.corpus_id='support' AND f.source_file_id=("
        " SELECT MAX(latest.source_file_id) FROM source_files latest "
        " WHERE latest.corpus_id=f.corpus_id AND latest.file_name=f.file_name) "
        "ORDER BY m.revision_id"
    ).fetchall()
    in_cases = {revision for record in records for revision in record["message_revision_ids"]}
    current_ids = {row["revision_id"] for row in current_messages}
    if not in_cases <= current_ids:
        raise ValueError("current cases contain non-current support message revisions")
    orphans = []
    empty_media = []
    for row in current_messages:
        try:
            has_media = bool(json.loads(row["media_refs_json"]))
        except (TypeError, ValueError) as error:
            raise ValueError(f"invalid media refs on revision {row['revision_id']}") from error
        if not row["body"].strip() and has_media:
            empty_media.append(row["revision_id"])
        if row["revision_id"] in in_cases:
            continue
        body = row["body"]
        orphans.append({
            "message_revision_id": row["revision_id"],
            "source_message_id": row["source_message_id"],
            "source_file": row["file_name"],
            "likely_question_or_problem": bool("?" in body or PROBLEM_TERMS.search(body)),
            "empty_body_with_media": bool(not body.strip() and has_media),
            "needs_followup": bool("?" in body or PROBLEM_TERMS.search(body)),
            "reference_search_hints": sorted({
                path for term, path in PRODUCT_TERMS.items() if term in body.lower()
            }),
        })
    if len(in_cases) + len(orphans) != len(current_messages):
        raise ValueError("support message coverage is incomplete")
    return {
        "report_kind": "private_support_gap_triage",
        "disclaimer": "Heuristics and reference paths are leads, not verified resolutions.",
        "case_count": len(records),
        "current_support_message_count": len(current_messages),
        "messages_in_current_cases_count": len(in_cases),
        "orphan_message_count": len(orphans),
        "empty_body_with_media_count": len(empty_media),
        "empty_body_with_media_revision_ids": empty_media,
        "likely_question_or_problem_count": sum(r["likely_question_or_problem"] for r in records),
        "needs_followup_count": sum(r["needs_followup"] for r in records),
        "orphan_needs_followup_count": sum(r["needs_followup"] for r in orphans),
        "suggested_tier_counts": dict(sorted(Counter(r["suggested_resolution_tier"] for r in records).items())),
        "cases": records,
        "orphan_messages": orphans,
    }


def write_report(report: dict, output_dir: Path) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    (output_dir / "support-gaps.json").write_text(
        json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    fields = [
        "case_id", "case_review_status", "message_revision_ids", "source_message_ids",
        "likely_question_or_problem", "suggested_resolution_tier",
        "resolution_signal_revision_ids", "resolution_review_state", "accepted_outcome",
        "needs_followup", "reference_search_hints",
    ]
    with (output_dir / "support-gaps.tsv").open("w", newline="", encoding="utf-8") as file:
        writer = csv.DictWriter(file, fieldnames=fields, delimiter="\t")
        writer.writeheader()
        for record in report["cases"]:
            writer.writerow({
                key: ",".join(map(str, value)) if isinstance(value, list) else value
                for key, value in record.items()
            })
    with (output_dir / "orphan-messages.tsv").open("w", newline="", encoding="utf-8") as file:
        orphan_fields = [
            "message_revision_id", "source_message_id", "source_file",
            "likely_question_or_problem", "empty_body_with_media", "needs_followup",
            "reference_search_hints",
        ]
        writer = csv.DictWriter(file, fieldnames=orphan_fields, delimiter="\t")
        writer.writeheader()
        for record in report["orphan_messages"]:
            writer.writerow({
                key: ",".join(value) if isinstance(value, list) else value
                for key, value in record.items()
            })


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=Path)
    parser.add_argument(
        "--output-dir", type=Path,
        default=Path(".raydium-debugger/gap-audit"),
        help="Private output directory (default: .raydium-debugger/gap-audit)",
    )
    args = parser.parse_args()
    database = args.database.resolve()
    if not database.is_file():
        parser.error(f"database does not exist: {database}")
    with sqlite3.connect(f"file:{database.as_posix()}?mode=ro", uri=True) as connection:
        report = build_report(connection)
    write_report(report, args.output_dir)
    print(
        f"Audited {report['case_count']} current support cases; "
        f"{report['current_support_message_count']} current support messages "
        f"({report['messages_in_current_cases_count']} in cases, "
        f"{report['orphan_message_count']} orphan); "
        f"{report['needs_followup_count']} cases and "
        f"{report['orphan_needs_followup_count']} orphan messages likely need follow-up. "
        f"Private report: {args.output_dir / 'support-gaps.json'}"
    )


if __name__ == "__main__":
    main()
