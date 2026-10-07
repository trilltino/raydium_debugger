"""Keep private Codex support triage running through checkpoints and quota resets.

Runs grouped cases first, then ungrouped messages. A file named `stop-worker`
in the private review directory requests a graceful stop between batches or
while waiting for quota. The process writes its status and log there.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]
PRIVATE = ROOT / ".raydium-debugger/ai-case-reviews"
STATUS = PRIVATE / "worker-status.json"
LOG = PRIVATE / "worker.log"
STOP = PRIVATE / "stop-worker"


def save_status(**fields) -> None:
    PRIVATE.mkdir(parents=True, exist_ok=True)
    payload = {"updated_at_utc": datetime.now(timezone.utc).isoformat(), **fields}
    temporary = STATUS.with_suffix(".json.tmp")
    temporary.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, STATUS)


def wait_or_stop(seconds: int) -> bool:
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        if STOP.exists():
            return False
        time.sleep(min(30, max(1, until - time.monotonic())))
    return not STOP.exists()


def run_once(stage: str, batch_size: int, max_batch_chars: int, model: str | None) -> tuple[int, dict | None, str]:
    command = [sys.executable, "-X", "utf8", str(ROOT / "scripts/classify-support-batch.py"),
               "--batch-size", str(1 if stage == "retry_invalid" else batch_size),
               "--max-batch-chars", str(max_batch_chars)]
    if stage != "retry_invalid":
        command.append("--skip-invalid")
    if stage == "cases":
        command.append("--cases-only")
    if model:
        command.extend(["--model", model])
    tail = ""
    summary = None
    with LOG.open("a", encoding="utf-8") as log:
        process = subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, text=True, encoding="utf-8",
                                   errors="replace")
        assert process.stdout
        for line in process.stdout:
            log.write(line)
            log.flush()
            tail = (tail + line)[-4000:]
            try:
                progress = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(progress, dict) and "processed" in progress:
                save_status(state="running", stage=stage, pid=process.pid,
                            progress=progress)
                if STOP.exists():
                    process.terminate()
                    process.wait()
                    return 2, summary, "stop-worker file present"
            elif isinstance(progress, dict) and "remaining" in progress:
                summary = progress
        exit_code = process.wait()
    return exit_code, summary, tail


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--batch-size", type=int, default=16)
    parser.add_argument("--max-batch-chars", type=int, default=120000)
    parser.add_argument("--quota-wait-seconds", type=int, default=3600)
    parser.add_argument("--model")
    args = parser.parse_args()
    if args.quota_wait_seconds < 60:
        parser.error("quota wait must be at least 60 seconds")
    PRIVATE.mkdir(parents=True, exist_ok=True)
    stage = "cases"
    stagnant_runs = 0
    previous_remaining = None
    transient_failures = 0
    with LOG.open("a", encoding="utf-8") as log:
        log.write(f"\n=== Started {datetime.now(timezone.utc).isoformat()} ===\n")
    while not STOP.exists():
        save_status(state="starting", stage=stage)
        code, summary, tail = run_once(stage, args.batch_size, args.max_batch_chars, args.model)
        if STOP.exists():
            break
        if code == 0 and summary is not None:
            transient_failures = 0
            remaining = summary["remaining"]
            if remaining == 0:
                if stage == "cases":
                    stage = "all"
                    stagnant_runs = 0
                    previous_remaining = None
                    continue
                if stage == "all":
                    stage = "retry_invalid"
                    stagnant_runs = 0
                    previous_remaining = None
                    continue
                save_status(state="complete", stage="all", summary=summary)
                return
            stagnant_runs = stagnant_runs + 1 if remaining == previous_remaining else 0
            previous_remaining = remaining
            if stagnant_runs >= 2:
                save_status(state="needs_review", stage=stage, summary=summary,
                            detail="Repeated passes made no coverage progress")
                return
            continue
        lowered = tail.lower()
        if any(phrase in lowered for phrase in ("usage limit", "rate limit", "quota", "try again at")):
            transient_failures = 0
            save_status(state="waiting_for_quota", stage=stage,
                        wait_seconds=args.quota_wait_seconds, detail=tail[-1200:])
            if not wait_or_stop(args.quota_wait_seconds):
                break
            continue
        transient_failures += 1
        if transient_failures <= 3:
            save_status(state="retrying_error", stage=stage,
                        retry=transient_failures, detail=tail[-1200:])
            if not wait_or_stop(300):
                break
            continue
        save_status(state="error", stage=stage, exit_code=code, detail=tail[-1200:])
        return
    save_status(state="stopped", stage=stage, detail="stop-worker file present")


if __name__ == "__main__":
    main()
