"""Join private OCR evidence to current support cases and orphan messages."""

from __future__ import annotations

import argparse
from collections import Counter
import csv
import hashlib
import json
from pathlib import Path
import re
import sqlite3


ROOT = Path(__file__).resolve().parents[1]
PROBLEM_HINT = re.compile(r"\b(error|failed|failure|unable|invalid|insufficient|rejected|stuck|cannot)\b", re.I)


def matching_image(export_root: Path, record: dict) -> bool:
    relative = record.get("relative_path")
    if not isinstance(relative, str) or "\\" in relative:
        return False
    parts = relative.split("/")
    if any(part in {"", ".", ".."} for part in parts):
        return False
    root = export_root.resolve()
    path = root.joinpath(*parts).resolve()
    return (path.is_relative_to(root) and path.is_file()
            and hashlib.sha256(path.read_bytes()).hexdigest() == record.get("sha256"))


def build(database: Path, ocr_file: Path, export_root: Path | None = None,
          notes_file: Path | None = None) -> dict:
    connection = sqlite3.connect(database.resolve().as_uri() + "?mode=ro", uri=True)
    try:
        rows = connection.execute("""
            SELECT m.revision_id, m.source_message_id, m.body, cm.case_id
              FROM message_revisions m
              JOIN source_files f ON f.source_file_id=m.source_file_id
              LEFT JOIN candidate_case_messages cm ON cm.revision_id=m.revision_id
              LEFT JOIN candidate_cases c ON c.case_id=cm.case_id
             WHERE m.corpus_id='support'
               AND f.source_file_id=(
                   SELECT MAX(f2.source_file_id) FROM source_files f2
                    WHERE f2.corpus_id=f.corpus_id AND f2.file_name=f.file_name)
               AND (c.case_id IS NULL OR (c.is_current=1 AND c.corpus_id='support'))
        """).fetchall()
    finally:
        connection.close()
    current = {revision: (message, body, case_id)
               for revision, message, body, case_id in rows}
    records = []
    stale = 0
    with ocr_file.open(encoding="utf-8") as file:
        for line in file:
            if line.strip():
                record = json.loads(line)
                source = current.get(record["revision_id"])
                if not source or source[0] != record["source_message_id"]:
                    continue
                if export_root is not None and not matching_image(export_root, record):
                    stale += 1
                    continue
                empty_source = not source[1].strip()
                problem_hint = bool(PROBLEM_HINT.search(record["text"]))
                priority = (
                    "high" if empty_source and (problem_hint or record["status"] != "ocr_text")
                    else "medium" if empty_source or problem_hint else "low"
                )
                records.append({
                    "case_id": source[2],
                    "message_revision_id": record["revision_id"],
                    "source_message_id": source[0],
                    "source_text_excerpt": source[1][:500],
                    "source_text_empty": empty_source,
                    "relative_path": record["relative_path"],
                    "attachment_sha256": record["sha256"],
                    "ocr_status": record["status"],
                    "ocr_method": record.get("method"),
                    "ocr_engine_version": record.get("engine_version"),
                    "ocr_line_count": record["line_count"],
                    "ocr_mean_confidence": record["mean_confidence"],
                    "ocr_text_excerpt": record["text"][:1200],
                    "sampled_video_frames": record.get("sampled_frames", []),
                    "sampled_pdf_pages": record.get("sampled_pages", []),
                    "ocr_problem_hint": problem_hint,
                    "review_priority": priority,
                    "visual_review_needed": True,
                })
    visual_notes = []
    if notes_file is not None and notes_file.is_file():
        with notes_file.open(encoding="utf-8") as file:
            for line in file:
                if not line.strip():
                    continue
                note = json.loads(line)
                source = current.get(note["revision_id"])
                if not source or source[0] != note["source_message_id"]:
                    continue
                if export_root is not None and not matching_image(export_root, note):
                    stale += 1
                    continue
                visual_notes.append({
                    "case_id": source[2],
                    "message_revision_id": note["revision_id"],
                    "source_message_id": source[0],
                    "relative_path": note["relative_path"],
                    "attachment_sha256": note["sha256"],
                    "reviewer": note["reviewer"],
                    "recorded_at": note["recorded_at"],
                    "status": note["status"],
                    "visual_note": note["visual_note"],
                })
    order = {"high": 0, "medium": 1, "low": 2}
    records.sort(key=lambda record: (order[record["review_priority"]], record["case_id"] or "~",
                                     record["message_revision_id"], record["relative_path"]))
    return {
        "report_kind": "private_media_evidence_review_queue",
        "disclaimer": "OCR is fallible and visual content is not fully described. Check the original image before accepting a resolution.",
        "media_count": len(records),
        "image_count": sum(Path(record["relative_path"]).suffix.lower()
                           not in {".mp4", ".mov", ".pdf"} for record in records),
        "video_count": sum(Path(record["relative_path"]).suffix.lower()
                           in {".mp4", ".mov"} for record in records),
        "scanned_pdf_count": sum(Path(record["relative_path"]).suffix.lower()
                                 == ".pdf" for record in records),
        "media_with_empty_source_text_count": sum(record["source_text_empty"] for record in records),
        "media_with_problem_hint_count": sum(record["ocr_problem_hint"] for record in records),
        "stale_or_missing_attachment_count": stale,
        "case_count": len({record["case_id"] for record in records if record["case_id"]}),
        "orphan_message_count": len({record["message_revision_id"] for record in records
                                     if record["case_id"] is None}),
        "status_counts": dict(sorted(Counter(record["ocr_status"] for record in records).items())),
        "review_priority_counts": dict(sorted(Counter(record["review_priority"] for record in records).items())),
        "evidence": records,
        "visual_note_count": len(visual_notes),
        "visual_notes": visual_notes,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", type=Path,
                        default=ROOT / ".raydium-debugger/support-knowledge.sqlite")
    parser.add_argument("--ocr", type=Path,
                        default=ROOT / ".raydium-debugger/attachment-audit/media-ocr.jsonl")
    parser.add_argument("--export-root", type=Path,
                        default=ROOT / ".raydium-debugger/ChatExport_2026-09-28")
    parser.add_argument("--notes", type=Path,
                        default=ROOT / ".raydium-debugger/attachment-audit/visual-notes.jsonl")
    parser.add_argument("--output", type=Path,
                        default=ROOT / ".raydium-debugger/attachment-audit/media-review-queue.json")
    parser.add_argument("--tsv-output", type=Path,
                        default=ROOT / ".raydium-debugger/attachment-audit/media-review-queue.tsv")
    args = parser.parse_args()
    report = build(args.database, args.ocr, args.export_root, args.notes)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n",
                           encoding="utf-8")
    fields = ("review_priority", "case_id", "message_revision_id", "source_message_id",
              "source_text_empty", "ocr_problem_hint", "ocr_status", "ocr_mean_confidence",
              "relative_path", "ocr_text_excerpt")
    args.tsv_output.parent.mkdir(parents=True, exist_ok=True)
    with args.tsv_output.open("w", newline="", encoding="utf-8") as file:
        writer = csv.DictWriter(file, fieldnames=fields, delimiter="\t")
        writer.writeheader()
        for item in report["evidence"]:
            row = {field: item[field] for field in fields}
            row["ocr_text_excerpt"] = " ".join(row["ocr_text_excerpt"].split())[:300]
            writer.writerow(row)
    print(json.dumps({key: report[key] for key in
                      ("media_count", "image_count", "video_count", "scanned_pdf_count",
                       "media_with_empty_source_text_count",
                       "media_with_problem_hint_count", "stale_or_missing_attachment_count", "case_count",
                       "orphan_message_count", "visual_note_count", "status_counts",
                       "review_priority_counts")},
                     indent=2))


if __name__ == "__main__":
    main()
