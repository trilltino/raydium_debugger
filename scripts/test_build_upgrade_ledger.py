"""Offline tests for the public upgrade ledger builder."""

import importlib.util
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location("build_upgrade_ledger", Path(__file__).with_name("build-upgrade-ledger.py"))
ledger_builder = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ledger_builder)


class UpgradeLedgerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.references = self.root / "refernce"
        self.docs = self.references / "raydium-docs-v1" / "reference" / "changelog"
        self.docs.mkdir(parents=True)
        self.commit = "a" * 40
        (self.references / "sources.json").write_text(json.dumps({"repositories": [
            {"directory": "raydium-docs-v1", "commit": self.commit},
            {"directory": "tino_radium_read", "commit": "b" * 40}
        ]}), encoding="utf-8")
        (self.docs / "2026-07-22-amm-v4-openbook-removal.mdx").write_text("---\ntitle: Release\n---\n# Release\nUpdated accounts.", encoding="utf-8")
        (self.docs / "2026-08-17-clmm-restricted-position-nft-freeze.mdx").write_text("# Release", encoding="utf-8")
        self.database = self.root / "knowledge.sqlite"
        with closing(sqlite3.connect(self.database)) as connection:
            with connection:
                connection.executescript("""
                CREATE TABLE source_files(source_file_id INTEGER PRIMARY KEY, file_name TEXT, imported_at INTEGER);
                CREATE TABLE message_revisions(revision_id INTEGER PRIMARY KEY, corpus_id TEXT,
                    source_file_id INTEGER, source_message_id TEXT, date_title TEXT, body TEXT);
                INSERT INTO source_files VALUES (1, 'messages_updates.html', 1);
                """)

    def add(self, message_id, date, body):
        with closing(sqlite3.connect(self.database)) as connection:
            with connection:
                connection.execute("INSERT INTO message_revisions(corpus_id,source_file_id,source_message_id,date_title,body) VALUES (?,?,?,?,?)",
                                   ("announcements", 1, str(message_id), date, body))

    def test_planned_and_live_stay_distinct(self):
        url = "https://docs.raydium.io/reference/changelog/2026-07-22-amm-v4-openbook-removal"
        self.add(25, "13.07.2026 15:37:48 UTC+00:00", "AMM v4 will be upgraded. " + url)
        self.add(26, "22.07.2026 14:37:10 UTC+00:00", "AMMv4 has successfully been upgraded. " + url)
        self.add(29, "14.08.2026 12:24:29 UTC+00:00", "The updates above have been delayed.")
        ledger = ledger_builder.build(self.database, self.references)
        announcements = [e for e in ledger["entries"] if e["source_message_id"]]
        self.assertEqual([e["status"] for e in announcements], ["planned", "live", "delayed"])
        self.assertEqual(ledger["entries"][0]["reference_commit"], self.commit)
        self.assertEqual(ledger["entries"][0]["reference_body"], "# Release\nUpdated accounts.")
        self.assertEqual(ledger["entries"][0]["date"], "2026-07-13")
        self.assertEqual(ledger["entries"][0]["announced_at"], "2026-07-13T15:37:48Z")
        self.assertEqual(ledger["entries"][1]["announced_at"], "2026-07-22T14:37:10Z")
        self.assertIsNone(ledger["entries"][2]["reference_path"])
        self.assertIsNone(ledger["entries"][2]["reference_body"])
        references = [e for e in ledger["entries"] if e["id"].startswith("reference:")]
        self.assertEqual(len(references), 2)
        self.assertEqual(references[0]["status"], "unknown")
        self.assertIsNone(references[0]["source_message_id"])
        self.assertIsNone(references[0]["announced_at"])
        self.assertEqual(references[0]["source_url"], url)

    def test_multiple_changelogs_preserved(self):
        self.add(28, "14.08.2026 10:53:38 UTC+00:00", "Upgrades set to go live. "
                 "https://docs.raydium.io/reference/changelog/2026-07-22-amm-v4-openbook-removal "
                 "https://docs.raydium.io/reference/changelog/2026-08-17-clmm-restricted-position-nft-freeze")
        entries = [e for e in ledger_builder.build(self.database, self.references)["entries"]
                   if e["source_message_id"]]
        self.assertEqual(len(entries), 2)
        self.assertEqual(len({e["id"] for e in entries}), 2)
        self.assertEqual({e["source_message_id"] for e in entries}, {"message28"})

    def test_missing_linked_page_is_visible_and_manifest_is_required(self):
        self.add(31, "24.08.2026 03:45:40 UTC+00:00", "Will be updated: "
                 "https://docs.raydium.io/reference/changelog/2026-08-24-launchlab-token2022-quote-mint")
        entry = ledger_builder.build(self.database, self.references)["entries"][0]
        self.assertIsNone(entry["reference_path"])
        self.assertIsNone(entry["reference_body"])
        (self.references / "sources.json").unlink()
        with self.assertRaisesRegex(ValueError, "missing source manifest"):
            ledger_builder.build(self.database, self.references)

    def test_missing_announcements_is_error(self):
        with self.assertRaisesRegex(ValueError, "announcements corpus is empty"):
            ledger_builder.build(self.database, self.references)

    def test_future_upgrade_notice_is_planned(self):
        self.assertEqual(ledger_builder.status_for(
            "CLMM upgrade (30 Sept): Anchor 1.0. Confirm the deployed program before relying on it."),
            "planned")


if __name__ == "__main__":
    unittest.main()
