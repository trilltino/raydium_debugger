"""Repeatable release executable benchmarks; reports contain no archive contents.

Run before and after changes with --label and the same --archive. Windows peak
working set includes the whole child process, sampled until it exits.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import statistics
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Memory(ctypes.Structure):
    _fields_ = [("cb", ctypes.c_ulong), ("PageFaultCount", ctypes.c_ulong)] + [
        (name, ctypes.c_size_t) for name in ["PeakWorkingSetSize", "WorkingSetSize",
        "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
        "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage"]]


def measure(executable, args, env):
    started = time.perf_counter()
    child = subprocess.Popen([str(executable), "support-knowledge", *map(str, args)],
                             env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    peak = 0
    while child.poll() is None:
        if os.name == "nt":
            memory = Memory()
            memory.cb = ctypes.sizeof(memory)
            if ctypes.windll.psapi.GetProcessMemoryInfo(ctypes.c_void_p(child._handle),
                                                       ctypes.byref(memory), memory.cb):
                peak = max(peak, memory.PeakWorkingSetSize)
        time.sleep(.005)
    out, err = child.communicate()
    if child.returncode:
        raise RuntimeError(f"benchmark command failed ({child.returncode}): {err.decode()[:200]}")
    return {"seconds": time.perf_counter() - started, "peak_bytes": peak}, out.decode()


def base58(data):
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    value, result = int.from_bytes(data, "big"), ""
    while value:
        value, digit = divmod(value, 58)
        result = alphabet[digit] + result
    return "1" * (len(data) - len(data.lstrip(b"\0"))) + result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", type=Path, default=Path("target/release/xtask.exe"))
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--baseline", type=Path,
                        help="Recorded report to compare; archive row counts must agree")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    work = Path("target/performance") / args.label
    work.mkdir(parents=True, exist_ok=True)
    frozen = work / "xtask.exe"
    shutil.copy2(args.exe, frozen)
    now = int(time.time()) - 60
    signatures = [base58(i.to_bytes(64, "big")) for i in range(1, 101)]

    class Rpc(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            if request["method"] == "getSignaturesForAddress":
                config = request["params"][1]
                start = signatures.index(config["before"]) + 1 if "before" in config else 0
                result = [{"signature": sig, "slot": 1000 + signatures.index(sig),
                           "blockTime": now} for sig in signatures[start:start + config["limit"]]]
            else:
                result = {"slot": 1000 + signatures.index(request["params"][0]),
                          "blockTime": now, "meta": {"err": None,
                          "logMessages": ["Program 11111111111111111111111111111111 success"]}}
            body = json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}).encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    rpc = ThreadingHTTPServer(("127.0.0.1", 0), Rpc)
    threading.Thread(target=rpc.serve_forever, daemon=True).start()
    env = dict(os.environ, TRITON_DEVNET_RPC_URL=f"http://127.0.0.1:{rpc.server_port}")
    records = {name: [] for name in ["import", "unchanged_import", "matching", "collector"]}
    outputs = []
    for run in range(args.runs):
        database = work / f"archive-{run}.sqlite"
        database.unlink(missing_ok=True)
        records["import"].append(measure(frozen, ["import-html", args.archive, database], env)[0])
        records["unchanged_import"].append(measure(frozen, ["import-html", args.archive, database], env)[0])
        with sqlite3.connect(database) as connection:
            outputs.append({table: connection.execute(f"SELECT count(*) FROM {table}").fetchone()[0]
                            for table in ["source_files", "message_revisions"]})
        timing, output = measure(frozen, ["evaluate-matches", "tests/fixtures/incident-match-benchmark.json"], env)
        timing["matching_report"] = json.loads(output)
        records["matching"].append(timing)
        observations = work / f"observations-{run}.sqlite"
        observations.unlink(missing_ok=True)
        records["collector"].append(measure(frozen, ["observations", "collect-rpc",
            "11111111111111111111111111111111", "--cluster", "devnet", "--since", now - 60,
            "--end", now + 1, "--database", observations, "--max-pages", 5], env)[0])
        with sqlite3.connect(observations) as connection:
            assert connection.execute("SELECT count(*) FROM recent_observations").fetchone()[0] == 100
    rpc.shutdown()
    summary = {name: {key: statistics.median(row[key] for row in rows)
                     for key in ["seconds", "peak_bytes"]} for name, rows in records.items()}
    report = {"label": args.label, "runs": args.runs, "summary": summary,
              "records": records, "compatibility_counts": outputs}
    report["executable_sha256"] = hashlib.sha256(frozen.read_bytes()).hexdigest()
    report["matching_latency_micros"] = {
        key: statistics.median(row["matching_report"][key] for row in records["matching"])
        for key in ["p50_micros", "p95_micros"]}
    if args.baseline:
        baseline = json.loads(args.baseline.read_text())
        if any(row != baseline["compatibility_counts"][0] for row in outputs):
            raise RuntimeError("Archive row counts differ from recorded baseline")
        report["comparison"] = {
            name: {key + "_change_percent":
                   (summary[name][key] / baseline["summary"][name][key] - 1) * 100
                   for key in ["seconds", "peak_bytes"]}
            for name in summary}
        report["baseline_report_sha256"] = hashlib.sha256(args.baseline.read_bytes()).hexdigest()
    (work / "report.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
