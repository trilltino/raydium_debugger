"""OCR private support screenshots, preserving source and file provenance.

Install the optional local engine in an ignored virtual environment:
    py -3 -m venv .raydium-debugger/ocr-venv
    .raydium-debugger/ocr-venv/Scripts/python -m pip install rapidocr onnxruntime
    .raydium-debugger/ocr-venv/Scripts/python scripts/interpret-support-media.py

The JSONL output is private evidence, not reviewed guidance. OCR can misread
numbers and program addresses, and cannot explain visual content without text.
"""

from __future__ import annotations

import argparse
from concurrent.futures import ProcessPoolExecutor, as_completed
import hashlib
from importlib.metadata import PackageNotFoundError, version
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_INVENTORY = ROOT / ".raydium-debugger/attachment-audit/support-attachments.json"
DEFAULT_EXPORT = ROOT / ".raydium-debugger/ChatExport_2026-09-28"
DEFAULT_OUTPUT = ROOT / ".raydium-debugger/attachment-audit/media-ocr.jsonl"
IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp", ".gif", ".bmp"}
VIDEO_EXTENSIONS = {".mp4", ".mov"}
MAX_BYTES = 12 * 1024 * 1024
MAX_VIDEO_BYTES = 64 * 1024 * 1024
MAX_TEXT = 12_000


def engine_version() -> str | None:
    try:
        return version("rapidocr")
    except PackageNotFoundError:
        return None


def source_path(export_root: Path, attachment: dict) -> Path | None:
    relative = attachment.get("relative_path")
    if not isinstance(relative, str) or not relative or "\\" in relative:
        return None
    parts = relative.split("/")
    if any(part in {"", ".", ".."} for part in parts):
        return None
    root = export_root.resolve()
    path = root.joinpath(*parts).resolve()
    return path if path.is_relative_to(root) else None


def key_for(attachment: dict) -> str:
    return f"{attachment['revision_id']}:{attachment['relative_path']}"


def existing_keys(output: Path) -> set[str]:
    if not output.is_file():
        return set()
    keys = set()
    with output.open(encoding="utf-8") as file:
        for line in file:
            if line.strip():
                record = json.loads(line)
                keys.add(record["key"])
    return keys


def interpret(attachment: dict, export_root: Path, engine) -> dict:
    path = source_path(export_root, attachment)
    record = {
        "key": key_for(attachment),
        "revision_id": attachment["revision_id"],
        "source_message_id": attachment["source_message_id"],
        "source_file": attachment["source_file"],
        "relative_path": attachment["relative_path"],
        "method": "rapidocr_local",
        "engine_version": engine_version(),
        "status": "unreadable",
        "sha256": None,
        "text": "",
        "line_count": 0,
        "mean_confidence": None,
        "visual_review_needed": True,
    }
    if path is None or not path.is_file():
        record["status"] = "missing_or_unsafe"
        return record
    if path.stat().st_size > MAX_BYTES:
        record["status"] = "over_size_limit"
        return record
    record["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
    try:
        result = engine(str(path))
        lines = list(result.txts or [])
        scores = list(result.scores or [])
        record["text"] = "\n".join(lines)[:MAX_TEXT]
        record["line_count"] = len(lines)
        record["mean_confidence"] = (
            round(sum(scores) / len(scores), 4) if scores else None
        )
        record["status"] = "ocr_text" if record["text"].strip() else "no_ocr_text"
    except Exception:
        # Parser/model exceptions can include private paths or raw data.
        record["status"] = "ocr_error"
    return record


def interpret_video(attachment: dict, export_root: Path, engine) -> dict:
    path = source_path(export_root, attachment)
    record = {
        "key": key_for(attachment),
        "revision_id": attachment["revision_id"],
        "source_message_id": attachment["source_message_id"],
        "source_file": attachment["source_file"],
        "relative_path": attachment["relative_path"],
        "method": "rapidocr_local_sampled_video_frames",
        "engine_version": engine_version(),
        "status": "video_unreadable",
        "sha256": None,
        "text": "",
        "line_count": 0,
        "mean_confidence": None,
        "sampled_frames": [],
        "visual_review_needed": True,
    }
    if path is None or not path.is_file():
        record["status"] = "missing_or_unsafe"
        return record
    if path.stat().st_size > MAX_VIDEO_BYTES:
        record["status"] = "over_size_limit"
        return record
    record["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
    try:
        import cv2
        video = cv2.VideoCapture(str(path))
        try:
            if not video.isOpened():
                return record
            fps = video.get(cv2.CAP_PROP_FPS)
            frames = video.get(cv2.CAP_PROP_FRAME_COUNT)
            duration = frames / fps if fps and fps > 0 and frames > 0 else 0
            # Sample a bounded portion; long videos still need direct review.
            end = min(duration, 60) if duration else 20
            count = min(8, max(1, int(end // 5) + 1))
            all_lines = []
            all_scores = []
            seen_lines = set()
            for index in range(count):
                second = end * index / max(1, count - 1)
                video.set(cv2.CAP_PROP_POS_MSEC, second * 1000)
                ok, frame = video.read()
                if not ok:
                    continue
                height, width = frame.shape[:2]
                if max(height, width) > 2000:
                    scale = 2000 / max(height, width)
                    frame = cv2.resize(frame, None, fx=scale, fy=scale,
                                       interpolation=cv2.INTER_AREA)
                result = engine(frame)
                lines = list(result.txts or [])
                scores = list(result.scores or [])
                record["sampled_frames"].append({
                    "second": round(second, 2), "line_count": len(lines),
                    "text_excerpt": "\n".join(lines)[:1200],
                })
                for line, score in zip(lines, scores):
                    if line not in seen_lines:
                        seen_lines.add(line)
                        all_lines.append(line)
                        all_scores.append(score)
            record["text"] = "\n".join(all_lines)[:MAX_TEXT]
            record["line_count"] = len(all_lines)
            record["mean_confidence"] = (
                round(sum(all_scores) / len(all_scores), 4) if all_scores else None
            )
            record["status"] = "video_ocr_text" if record["text"].strip() else "no_video_ocr_text"
        finally:
            video.release()
    except Exception:
        record["status"] = "video_ocr_error"
    return record


def interpret_scanned_pdf(attachment: dict, export_root: Path, engine) -> dict:
    path = source_path(export_root, attachment)
    record = {
        "key": key_for(attachment),
        "revision_id": attachment["revision_id"],
        "source_message_id": attachment["source_message_id"],
        "source_file": attachment["source_file"],
        "relative_path": attachment["relative_path"],
        "method": "rapidocr_local_rendered_pdf_pages",
        "engine_version": engine_version(),
        "status": "pdf_ocr_error",
        "sha256": None,
        "text": "",
        "line_count": 0,
        "mean_confidence": None,
        "sampled_pages": [],
        "visual_review_needed": True,
    }
    if path is None or not path.is_file():
        record["status"] = "missing_or_unsafe"
        return record
    if path.stat().st_size > MAX_BYTES:
        record["status"] = "over_size_limit"
        return record
    record["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
    try:
        import pymupdf
        document = pymupdf.open(path)
        try:
            lines = []
            scores = []
            for page_index in range(min(len(document), 10)):
                page = document[page_index]
                scale = min(1.5, 2000 / max(page.rect.width, page.rect.height))
                pixmap = page.get_pixmap(matrix=pymupdf.Matrix(scale, scale), alpha=False)
                result = engine(pixmap.tobytes("png"))
                page_lines = list(result.txts or [])
                page_scores = list(result.scores or [])
                record["sampled_pages"].append({
                    "page": page_index + 1, "line_count": len(page_lines),
                    "text_excerpt": "\n".join(page_lines)[:1200],
                })
                lines.extend(page_lines)
                scores.extend(page_scores)
            record["text"] = "\n".join(lines)[:MAX_TEXT]
            record["line_count"] = len(lines)
            record["mean_confidence"] = (
                round(sum(scores) / len(scores), 4) if scores else None
            )
            record["status"] = "pdf_ocr_text" if record["text"].strip() else "no_pdf_ocr_text"
        finally:
            document.close()
    except Exception:
        record["status"] = "pdf_ocr_error"
    return record


def interpret_media(attachment: dict, export_root: Path, engine) -> dict:
    if attachment.get("extension") == ".pdf":
        return interpret_scanned_pdf(attachment, export_root, engine)
    if attachment.get("extension") in VIDEO_EXTENSIONS:
        return interpret_video(attachment, export_root, engine)
    return interpret(attachment, export_root, engine)


def run(inventory: dict, export_root: Path, output: Path, engine, limit: int = 0) -> dict:
    if inventory.get("report_kind") != "private_support_attachment_inventory":
        raise ValueError("unexpected attachment inventory")
    attachments = [
        entry for entry in inventory["attachments"]
        if (entry.get("extension") in IMAGE_EXTENSIONS | VIDEO_EXTENSIONS
            and entry.get("status") == "unprocessed_media")
        or (entry.get("extension") == ".pdf"
            and entry.get("status") == "no_extractable_text")
    ]
    seen = existing_keys(output)
    written = 0
    counts = {}
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("a", encoding="utf-8") as file:
        for attachment in attachments:
            if key_for(attachment) in seen:
                continue
            record = interpret_media(attachment, export_root, engine)
            file.write(json.dumps(record, ensure_ascii=False) + "\n")
            file.flush()
            seen.add(record["key"])
            written += 1
            counts[record["status"]] = counts.get(record["status"], 0) + 1
            if limit and written >= limit:
                break
    return {"media_references": len(attachments), "recorded": len(seen),
            "new_records": written, "new_status_counts": counts, "output": str(output)}


_WORKER_ENGINE = None


def init_worker() -> None:
    global _WORKER_ENGINE
    from rapidocr import RapidOCR
    _WORKER_ENGINE = make_engine(RapidOCR)


def make_engine(factory):
    # Bound ONNX threads per worker to avoid CPU oversubscription on archive runs.
    return factory(params={
        "EngineConfig.onnxruntime.intra_op_num_threads": 1,
        "EngineConfig.onnxruntime.inter_op_num_threads": 1,
        "Global.log_level": "error",
    })


def worker_interpret(attachment: dict, export_root: Path) -> dict:
    return interpret_media(attachment, export_root, _WORKER_ENGINE)


def run_parallel(inventory: dict, export_root: Path, output: Path,
                 workers: int, limit: int = 0) -> dict:
    if inventory.get("report_kind") != "private_support_attachment_inventory":
        raise ValueError("unexpected attachment inventory")
    attachments = [
        entry for entry in inventory["attachments"]
        if (entry.get("extension") in IMAGE_EXTENSIONS | VIDEO_EXTENSIONS
            and entry.get("status") == "unprocessed_media")
        or (entry.get("extension") == ".pdf"
            and entry.get("status") == "no_extractable_text")
    ]
    seen = existing_keys(output)
    pending = [entry for entry in attachments if key_for(entry) not in seen]
    if limit:
        pending = pending[:limit]
    counts = {}
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("a", encoding="utf-8") as file:
        with ProcessPoolExecutor(max_workers=workers, initializer=init_worker) as pool:
            futures = [pool.submit(worker_interpret, entry, export_root) for entry in pending]
            for future in as_completed(futures):
                record = future.result()
                file.write(json.dumps(record, ensure_ascii=False) + "\n")
                file.flush()
                seen.add(record["key"])
                counts[record["status"]] = counts.get(record["status"], 0) + 1
    return {"media_references": len(attachments), "recorded": len(seen),
            "new_records": len(pending), "new_status_counts": counts, "output": str(output)}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=Path, default=DEFAULT_INVENTORY)
    parser.add_argument("--export-root", type=Path, default=DEFAULT_EXPORT)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--limit", type=int, default=0, help="New images to process; 0 means all")
    parser.add_argument("--workers", type=int, default=1, help="Independent local OCR workers")
    args = parser.parse_args()
    if args.limit < 0 or not 1 <= args.workers <= 8:
        parser.error("--limit must be nonnegative and --workers must be 1 to 8")
    try:
        from rapidocr import RapidOCR
    except ImportError:
        parser.error("install rapidocr and onnxruntime in a local virtual environment")
    inventory = json.loads(args.inventory.read_text(encoding="utf-8"))
    if args.workers == 1:
        result = run(inventory, args.export_root, args.output, make_engine(RapidOCR), args.limit)
    else:
        result = run_parallel(inventory, args.export_root, args.output,
                              args.workers, args.limit)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
