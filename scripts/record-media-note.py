"""Record a private visual interpretation for an exact support attachment.

The note is unverified review evidence, not a support-case approval. Supply note
text in a UTF-8 file so it does not appear in shell history or process arguments.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def add_note(inventory: dict, export_root: Path, output: Path,
             revision_id: int, relative_path: str, note: str, reviewer: str) -> dict:
    matches = [entry for entry in inventory.get("attachments", [])
               if entry.get("revision_id") == revision_id
               and entry.get("relative_path") == relative_path]
    if len(matches) != 1:
        raise ValueError("attachment not uniquely present in the current inventory")
    entry = matches[0]
    root = export_root.resolve()
    parts = relative_path.split("/")
    if "\\" in relative_path or any(part in {"", ".", ".."} for part in parts):
        raise ValueError("unsafe attachment path")
    path = root.joinpath(*parts).resolve()
    if not path.is_relative_to(root) or not path.is_file():
        raise ValueError("attachment unavailable or outside export")
    note = note.strip()
    reviewer = reviewer.strip()
    if len(note) < 12 or len(note) > 4000 or not reviewer:
        raise ValueError("note must be 12–4000 characters and reviewer is required")
    record = {
        "revision_id": revision_id,
        "source_message_id": entry["source_message_id"],
        "relative_path": relative_path,
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "reviewer": reviewer,
        "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "visual_note": note,
        "status": "unverified_visual_interpretation",
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("a", encoding="utf-8") as file:
        file.write(json.dumps(record, ensure_ascii=False) + "\n")
    return record


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("revision_id", type=int)
    parser.add_argument("relative_path")
    parser.add_argument("note_file", type=Path)
    parser.add_argument("reviewer")
    parser.add_argument("--inventory", type=Path,
                        default=ROOT / ".raydium-debugger/attachment-audit/support-attachments.json")
    parser.add_argument("--export-root", type=Path,
                        default=ROOT / ".raydium-debugger/ChatExport_2026-09-28")
    parser.add_argument("--output", type=Path,
                        default=ROOT / ".raydium-debugger/attachment-audit/visual-notes.jsonl")
    args = parser.parse_args()
    inventory = json.loads(args.inventory.read_text(encoding="utf-8"))
    record = add_note(inventory, args.export_root, args.output,
                      args.revision_id, args.relative_path,
                      args.note_file.read_text(encoding="utf-8"), args.reviewer)
    print(f"Recorded unverified visual note for {record['source_message_id']} / {record['relative_path']}")


if __name__ == "__main__":
    main()
