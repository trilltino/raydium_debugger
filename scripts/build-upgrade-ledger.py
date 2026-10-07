"""Build a private, offline ledger of public Raydium upgrade announcements.

This does not infer fixes or deployment from a changelog's publication date.
Each entry preserves the announcement text and optional pinned documentation path.
"""

import argparse
from contextlib import closing
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import sqlite3
import sys


DOC_LINK = re.compile(r"https?://docs\.raydium\.io/reference/changelog/(20\d\d-\d\d-\d\d-[a-z0-9-]+)")
MAX_BODY = 8192
MAX_SUMMARY = 240
MAX_REFERENCE_BODY = 16384
LIVE = re.compile(r"\b(?:is live|now live|has successfully been upgraded|has been upgraded|has been deployed|was deployed|just deployed|have been completed|upgrade is live|upgrades have been completed)\b", re.I)
DELAYED = re.compile(r"\b(?:delayed|postponed)\b", re.I)
PLANNED = re.compile(r"\b(?:will be|will\s+be\s+updated|going live|set to go live|pending|upcoming|soon|upgrade\s*\(\d+|upgrading|effective on)\b", re.I)


def status_for(text):
    if DELAYED.search(text):
        return "delayed"
    if LIVE.search(text):
        return "live"
    if PLANNED.search(text):
        return "planned"
    return "unknown"


def bounded(text, limit):
    text = text.strip()
    return text if len(text) <= limit else text[: limit - 1].rstrip() + "…"


def date_for(date_title):
    return datetime.strptime(date_title[:10], "%d.%m.%Y").date().isoformat()


def announced_at_for(date_title):
    parsed = datetime.strptime(date_title, "%d.%m.%Y %H:%M:%S UTC%z")
    return parsed.astimezone(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def changelog_slugs(body):
    return list(dict.fromkeys(DOC_LINK.findall(body)))


def reference_body_for(path):
    text = path.read_text(encoding="utf-8-sig")
    if text.startswith("---\n"):
        end = text.find("\n---", 4)
        if end != -1:
            text = text[end + 4:]
    return bounded(text, MAX_REFERENCE_BODY)


def reference_title_for(path):
    text = path.read_text(encoding="utf-8-sig")
    match = re.search(r'^title:\s*["\']?([^\n"\']+)', text, re.M)
    return bounded(match.group(1).strip() if match else path.stem, MAX_SUMMARY)


def load_manifest(reference_root):
    manifest_path = reference_root / "sources.json"
    if not manifest_path.is_file():
        raise ValueError(f"missing source manifest: {manifest_path}")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8-sig"))
    docs = next((repo for repo in manifest.get("repositories", [])
                 if repo.get("directory") == "raydium-docs-v1"), None)
    if not docs or not re.fullmatch(r"[0-9a-f]{40}", docs.get("commit", "")):
        raise ValueError("source manifest lacks pinned raydium-docs-v1 commit")
    docs_root = reference_root / "raydium-docs-v1" / "reference" / "changelog"
    if not docs_root.is_dir() or not list(docs_root.glob("*.mdx")):
        raise ValueError(f"missing official changelog pages: {docs_root}")
    return docs, docs_root


def load_announcements(database):
    if not database.is_file():
        raise ValueError(f"missing support database: {database}")
    with closing(sqlite3.connect(database.resolve().as_uri() + "?mode=ro", uri=True)) as connection:
        rows = connection.execute("""
            SELECT m.source_message_id, m.date_title, m.body, f.file_name
              FROM message_revisions m
              JOIN source_files f ON f.source_file_id = m.source_file_id
             WHERE m.corpus_id = 'announcements'
               AND m.revision_id = (
                 SELECT m2.revision_id FROM message_revisions m2
                  JOIN source_files f2 ON f2.source_file_id = m2.source_file_id
                 WHERE m2.corpus_id = m.corpus_id
                   AND m2.source_message_id = m.source_message_id
                 ORDER BY f2.imported_at DESC, m2.revision_id DESC LIMIT 1)
             ORDER BY CAST(m.source_message_id AS INTEGER), m.source_message_id
        """).fetchall()
    if not rows:
        raise ValueError("announcements corpus is empty; import messages_updates.html first")
    for message_id, date, body, file_name in rows:
        if not message_id.isdigit() or not date or not body.strip() or file_name != "messages_updates.html":
            raise ValueError(f"unexpected announcement source record: {message_id!r}, {file_name!r}")
    return rows


def build(database, reference_root):
    docs, docs_root = load_manifest(reference_root)
    known_slugs = {path.stem for path in docs_root.glob("*.mdx")}
    entries = []
    for source_message_id, date_title, body, _file_name in load_announcements(database):
        slugs = []
        for raw_slug in changelog_slugs(body):
            slug = raw_slug
            # Telegram HTML parsing can concatenate a numbered next section
            # directly to a URL, e.g. ...platform-config2. CLMM: ...
            if slug not in known_slugs:
                matches = [candidate for candidate in known_slugs
                           if slug.startswith(candidate) and 1 <= len(slug) - len(candidate) <= 4]
                if matches:
                    slug = max(matches, key=len)
            if slug not in slugs:
                slugs.append(slug)
        # A generic changelog URL, an anchor to an unreleased section, or no URL
        # cannot be safely mapped to a particular release document.
        for slug in slugs or [None]:
            relative = f"reference/changelog/{slug}.mdx" if slug in known_slugs else None
            if slug and not relative:
                print(f"warning: announcement {source_message_id} links absent changelog slug {slug}; reference fields set to null", file=sys.stderr)
            source_url = f"https://t.me/RaydiumDeveloperUpdates/{source_message_id}"
            summary = bounded(next((line.strip() for line in body.splitlines() if line.strip()), ""), MAX_SUMMARY)
            entries.append({
                "id": f"announcement:message{source_message_id}" + (f":{slug}" if len(slugs) > 1 else ""),
                "source_message_id": f"message{source_message_id}",
                "date": date_for(date_title),
                "announced_at": announced_at_for(date_title),
                "status": status_for(body),
                "summary": summary,
                "body": bounded(body, MAX_BODY),
                "source_url": source_url,
                "reference_repo": "raydium-docs-v1" if relative else None,
                "reference_commit": docs["commit"] if relative else None,
                "reference_path": relative,
                "reference_body": reference_body_for(docs_root / f"{slug}.mdx") if relative else None,
            })
    for path in sorted(docs_root.glob("*.mdx")):
        slug = path.stem
        if not re.fullmatch(r"20\d\d-\d\d-\d\d-[a-z0-9-]+", slug):
            raise ValueError(f"unrecognized official changelog filename: {path.name}")
        relative = f"reference/changelog/{path.name}"
        reference_body = reference_body_for(path)
        entries.append({
            "id": f"reference:{slug}",
            "source_message_id": None,
            "date": slug[:10],
            "announced_at": None,
            "status": "unknown",
            "summary": reference_title_for(path),
            "body": bounded(reference_body, MAX_BODY),
            "source_url": f"https://docs.raydium.io/reference/changelog/{slug}",
            "reference_repo": "raydium-docs-v1",
            "reference_commit": docs["commit"],
            "reference_path": relative,
            "reference_body": reference_body,
        })
    return {"schema_version": 1, "entries": entries}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", type=Path, default=Path(".raydium-debugger/support-knowledge.sqlite"))
    parser.add_argument("--references", type=Path, default=Path("refernce"))
    parser.add_argument("--output", type=Path, default=Path(".raydium-debugger/knowledge/updates.generated.json"))
    args = parser.parse_args()
    try:
        ledger = build(args.database, args.references)
    except (ValueError, sqlite3.Error, OSError, json.JSONDecodeError) as exc:
        parser.exit(1, f"upgrade ledger: {exc}\n")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(ledger, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(args.output), "entries": len(ledger["entries"]),
                      "announcements": len({e["source_message_id"] for e in ledger["entries"] if e["source_message_id"]}),
                      "unmapped_documents": sum(e["reference_path"] is None for e in ledger["entries"])}, indent=2))


if __name__ == "__main__":
    main()
