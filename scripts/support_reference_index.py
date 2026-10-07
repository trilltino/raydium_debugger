"""Index pinned local Raydium docs and code for evidence-linked case research.

The index contains only repository text. Support messages stay in the private
builder database. A matching reference can explain a product or a possible fix;
it is not evidence that a particular support reporter's issue was resolved.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from datetime import date, datetime
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess
from urllib.parse import quote


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REFERENCE_ROOT = ROOT / "refernce"
DEFAULT_INDEX = ROOT / ".raydium-debugger/reference-index.jsonl"
DEFAULT_UPGRADE_LEDGER = ROOT / ".raydium-debugger/knowledge/updates.generated.json"
EXTENSIONS = {".md", ".mdx", ".rst", ".txt", ".rs", ".ts", ".tsx", ".js",
              ".jsx", ".json", ".toml", ".yaml", ".yml", ".sol", ".py"}
SKIP_NAMES = {"AGENTS.md", "CLAUDE.md", "yarn.lock", "package-lock.json",
              "pnpm-lock.yaml", "Cargo.lock", "docs.json", "tsconfig.tsbuildinfo"}
SKIP_DIRS = {".git", "node_modules", "target", "dist", "build", ".next", "coverage"}
LOCALES = {"ar", "de", "es", "fr", "id", "ja", "ko", "pt", "ru", "tr", "vi", "zh", "zh-Hant"}
STOP = set("""a about after all also an and any are as at be been before but by can
could did do does for from had has have how i if in into is it its may more my no
not of on or our please should so some that the their them there these they this
those to up us use using was we were what when where which who why will with would
you your raydium docs documentation github com https http error issue help thanks
hello hi team user users one two new old now then here see need problem""".split())
STOP.update({"work", "working"})
TOKEN_RE = re.compile(r"[a-z][a-z0-9]{2,}")
TOPIC_TERMS = {"pool", "pools", "liquidity", "withdraw", "deposit", "swap",
               "wallet", "perps", "clmm", "cpmm", "ammv4", "launchlab",
               "farm", "stake", "staking", "claim", "harvest", "token2022",
               "sdk", "api", "rpc", "timestamp", "cache", "slippage",
               "compute", "priority", "fee", "fees", "position", "nft"}
FOCUS_TERMS = {"pool", "pools", "liquidity", "withdraw", "deposit", "swap",
               "perps", "clmm", "cpmm", "ammv4", "launchlab", "farm",
               "staking", "position", "token2022"}
ISSUE_PATH_TERMS = {"cache", "timestamp", "slippage", "compute", "priority"}
PRODUCTS = {"cpmm", "clmm", "launchlab", "perps", "ammv4", "stable"}


def terms(value: str) -> set[str]:
    value = re.sub(r"([a-z])([A-Z])", r"\1 \2", value)
    value = value.lower().replace("token-2022", "token2022").replace("amm-v4", "ammv4")
    value = re.sub(r"\bamm\s*v4\b", "ammv4", value)
    return {word for word in TOKEN_RE.findall(value) if word not in STOP}


def _tracked_paths(repo: Path) -> list[Path]:
    result = subprocess.run(["git", "-C", str(repo), "ls-files", "-z"],
                            capture_output=True, check=False)
    if result.returncode == 0:
        names = [Path(name.decode("utf-8", "surrogateescape"))
                 for name in result.stdout.split(b"\0") if name]
    else:
        names = [path.relative_to(repo) for path in repo.rglob("*") if path.is_file()
                 and not set(path.relative_to(repo).parts).intersection(SKIP_DIRS)]
    return sorted(names, key=lambda path: path.as_posix())


def _chunks(lines: list[str], *, size: int = 60, overlap: int = 8):
    if not lines:
        return
    for start in range(0, len(lines), size - overlap):
        end = min(start + size, len(lines))
        chunk = "\n".join(lines[start:end]).strip()
        if chunk:
            yield start + 1, end, chunk
        if end == len(lines):
            break


def build_index(reference_root: Path = DEFAULT_REFERENCE_ROOT,
                output: Path = DEFAULT_INDEX) -> dict:
    """Write deterministic JSONL chunks from the repos pinned by sources.json."""
    reference_root = Path(reference_root).resolve()
    manifest = json.loads((reference_root / "sources.json").read_text(encoding="utf-8-sig"))
    records = []
    skipped = Counter()
    indexed_files = 0
    for source in manifest["repositories"]:
        name = source["directory"]
        if not re.fullmatch(r"[A-Za-z0-9_.-]+", name):
            raise ValueError(f"Unsafe repository directory: {name!r}")
        repo = (reference_root / name).resolve()
        if not repo.is_dir() or not repo.is_relative_to(reference_root):
            raise FileNotFoundError(f"Pinned repository missing: {repo}")
        # Never cite a revision other than the one actually checked out.
        head = subprocess.run(["git", "-C", str(repo), "rev-parse", "HEAD"],
                              capture_output=True, text=True, check=False)
        if head.returncode != 0 or not re.fullmatch(r"[0-9a-f]{40}", head.stdout.strip()):
            raise ValueError(f"Cannot verify pinned Git HEAD for {name}: {head.stderr.strip()}")
        if head.stdout.strip() != source["commit"]:
            raise ValueError(f"Pinned commit mismatch for {name}: {head.stdout.strip()}")
        for relative in _tracked_paths(repo):
            if relative.name in SKIP_NAMES or relative.suffix.lower() not in EXTENSIONS:
                skipped["extension_or_metadata"] += 1
                continue
            if set(relative.parts).intersection(SKIP_DIRS):
                skipped["generated"] += 1
                continue
            path = repo / relative
            if not path.is_file() or not path.resolve().is_relative_to(repo):
                skipped["missing_or_external"] += 1
                continue
            if path.stat().st_size > 512_000:
                skipped["oversize"] += 1
                continue
            try:
                body = path.read_text(encoding="utf-8")
            except (UnicodeError, OSError):
                skipped["non_utf8"] += 1
                continue
            if "\0" in body:
                skipped["binary"] += 1
                continue
            indexed_files += 1
            path_posix = relative.as_posix()
            kind = "code" if relative.suffix.lower() in {".rs", ".ts", ".tsx", ".js", ".jsx", ".sol", ".py"} else "docs"
            url_base = source["remote"].removesuffix(".git").rstrip("/")
            authority = "official" if url_base.startswith("https://github.com/raydium-io/") else "community"
            for first, last, chunk in _chunks(body.splitlines()):
                digest = hashlib.sha256(f"{name}\0{source['commit']}\0{path_posix}\0{first}\0{last}\0{chunk}".encode()).hexdigest()[:20]
                records.append({
                    "id": f"ref-{digest}", "repo": name, "commit": source["commit"],
                    "path": path_posix, "start_line": first, "end_line": last,
                    "kind": kind, "authority": authority,
                    "collected_on": manifest.get("collectedOn"), "text": chunk,
                    "url": f"{url_base}/blob/{source['commit']}/{quote(path_posix, safe='/')}#L{first}-L{last}",
                })
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", encoding="utf-8", newline="\n") as file:
        for record in records:
            file.write(json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n")
    return {"repositories": len(manifest["repositories"]), "files": indexed_files,
            "chunks": len(records), "skipped": dict(sorted(skipped.items())),
            "output": str(output),
            "fingerprint": hashlib.sha256("\n".join(row["id"] for row in records).encode()).hexdigest()}


class ReferenceIndex:
    def __init__(self, records: list[dict]):
        self.records = records
        self.postings: dict[str, list[int]] = defaultdict(list)
        self.path_terms: list[set[str]] = []
        self.text_terms: list[set[str]] = []
        for index, row in enumerate(records):
            path_words = terms(row["path"].replace("/", " ").replace("_", " "))
            body_words = terms(row["text"])
            self.path_terms.append(path_words)
            self.text_terms.append(body_words)
            for word in path_words | body_words:
                self.postings[word].append(index)


def load_index(path: Path = DEFAULT_INDEX) -> ReferenceIndex:
    with Path(path).open(encoding="utf-8") as file:
        return ReferenceIndex([json.loads(line) for line in file if line.strip()])


def query(index: ReferenceIndex, text: str, limit: int = 8) -> list[dict]:
    """Rank matching pinned chunks; return no citation for a zero-overlap query."""
    if limit <= 0:
        return []
    words = terms(text)
    query_products = words & PRODUCTS
    if not words:
        return []
    total = len(index.records)
    # Long threads have many incidental words. Use the most discriminating
    # query terms so one verbose message does not swamp the useful keywords.
    available = {word for word in words if word in index.postings}
    focus = available & FOCUS_TERMS
    anchors = sorted(available & TOPIC_TERMS)
    rare = sorted(available - set(anchors),
                  key=lambda word: (len(index.postings[word]), word))
    words = (anchors + rare)[:24]
    scores = defaultdict(float)
    matched = defaultdict(int)
    for word in words:
        idf = math.log1p((total + 1) / (len(index.postings[word]) + 1))
        for position in index.postings[word]:
            scores[position] += idf * (2.2 if word in index.path_terms[position] else 1.0)
            matched[position] += 1
    ranked = []
    for position, score in scores.items():
        if matched[position] < 2 and len(words) > 1:
            continue
        row = index.records[position]
        path_products = index.path_terms[position] & PRODUCTS
        body_products = index.text_terms[position] & PRODUCTS
        if query_products and not query_products.intersection(path_products | body_products):
            continue
        # Thread language is broad enough to match unrelated white papers and
        # overview pages. A cited source needs a topic-bearing path match.
        path_overlap = set(words) & index.path_terms[position]
        if len(words) > 1 and len(path_overlap) < 2 and not (path_overlap & TOPIC_TERMS):
            continue
        if focus and not (path_overlap & (focus | ISSUE_PATH_TERMS)):
            continue
        # English is the canonical docs tree; prefer it when translations match.
        if row["repo"] == "raydium-docs-v1" and row["path"].split("/")[0] in LOCALES:
            score *= 0.55
        if row["authority"] == "community":
            score *= 0.7
        if "tests" in row["path"].split("/"):
            score *= 0.6
        score *= min(1.0, matched[position] / max(2, min(6, len(words))))
        ranked.append((score, row["id"], row))
    ranked.sort(key=lambda item: (-item[0], item[1]))
    # Avoid returning eight adjacent chunks from the same page.
    selected, per_file = [], Counter()
    for score, _, row in ranked:
        file_key = (row["repo"], row["path"])
        if per_file[file_key] >= 2:
            continue
        selected.append({**row, "score": round(score, 4)})
        per_file[file_key] += 1
        if len(selected) >= limit:
            break
    return selected


retrieve = query


def query_upgrades(ledger_path: Path, text: str, message_date: str | None,
                   limit: int = 3) -> list[dict]:
    """Return dated announcement context, never a claim about a case outcome.

    A planned announcement stays planned. A live announcement after the case is
    explicitly marked as later context rather than evidence of historical state.
    """
    if limit <= 0 or not Path(ledger_path).is_file():
        return []
    ledger = json.loads(Path(ledger_path).read_text(encoding="utf-8-sig"))
    query_words = terms(text)
    query_products = query_words & PRODUCTS
    case_day = None
    if message_date:
        for date_format in ("%Y-%m-%d", "%d.%m.%Y"):
            try:
                case_day = datetime.strptime(message_date[:10], date_format).date()
                break
            except ValueError:
                continue
    candidates = []
    for entry in ledger.get("entries", []):
        source = entry.get("reference_body") or entry.get("body") or entry.get("summary") or ""
        path = entry.get("reference_path") or ""
        path_words = terms(path)
        entry_products = (path_words | terms(entry.get("summary") or "")) & PRODUCTS
        if query_products and not query_products.intersection(entry_products):
            continue
        if not query_products and len(query_words & path_words) < 2:
            continue
        overlap = query_words & terms(f"{entry.get('summary', '')} {source} {path}")
        if len(overlap) < 2:
            continue
        status = entry.get("status") or "unknown"
        try:
            announcement_day = date.fromisoformat(entry["date"][:10])
        except (KeyError, ValueError, TypeError):
            announcement_day = None
        if case_day and announcement_day and (announcement_day - case_day).days > 90:
            continue
        if case_day and announcement_day and announcement_day > case_day:
            chronology = "announced_after_case"
        elif status == "live" and case_day and announcement_day:
            chronology = "live_announcement_by_case_date"
        elif status == "planned":
            chronology = "planned_announcement_only"
        else:
            chronology = "status_at_case_unverified"
        # Match on multiple specific words and favor product agreement.
        score = len(overlap) + (3 if query_products & entry_products else 0)
        candidates.append((score, entry.get("id", ""), {
            "id": entry.get("id"), "date": entry.get("date"), "status": status,
            "chronology": chronology, "summary": entry.get("summary"),
            "body_excerpt": source[:3500], "source_url": entry.get("source_url"),
            "reference_repo": entry.get("reference_repo"),
            "reference_commit": entry.get("reference_commit"),
            "reference_path": entry.get("reference_path"),
            "evidence_role": "dated_product_context_not_case_outcome",
            "score": score,
        }))
    candidates.sort(key=lambda item: (-item[0], item[1]))
    return [candidate for _, _, candidate in candidates[:limit]]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    build = sub.add_parser("build")
    build.add_argument("--reference-root", type=Path, default=DEFAULT_REFERENCE_ROOT)
    build.add_argument("--output", type=Path, default=DEFAULT_INDEX)
    search = sub.add_parser("query")
    search.add_argument("text")
    search.add_argument("--index", type=Path, default=DEFAULT_INDEX)
    search.add_argument("--limit", type=int, default=8)
    args = parser.parse_args()
    result = build_index(args.reference_root, args.output) if args.command == "build" else query(load_index(args.index), args.text, args.limit)
    print(json.dumps(result, ensure_ascii=True, indent=2))


if __name__ == "__main__":
    main()
