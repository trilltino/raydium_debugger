"""Focused checks for private support to public reference suggestions."""

import importlib.util
from pathlib import Path
import sqlite3
import unittest


SCRIPT = Path(__file__).with_name("link-support-references.py")
spec = importlib.util.spec_from_file_location("link_support_references", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ReferenceMatchingTests(unittest.TestCase):
    def test_planned_is_never_fix_live_is_check_and_unrelated_excluded(self):
        database = sqlite3.connect(":memory:")
        database.execute("CREATE TABLE message_revisions (revision_id INTEGER, corpus_id TEXT, body TEXT, date_title TEXT)")
        database.execute(
            "INSERT INTO message_revisions VALUES (1,'support',?,?)",
            ("CLMM limit order settlement fails with OrderPhaseSaturated after upgrade", "20.05.2026 09:00:00 UTC+00:00"),
        )
        audit = {
            "report_kind": "private_support_gap_triage",
            "cases": [{"case_id": "case-1", "needs_followup": True, "message_revision_ids": [1]}],
            "orphan_messages": [],
        }
        ledger = {"entries": [
            {"id": "planned", "date": "2026-05-08", "status": "planned", "body": "CLMM limit order settlement OrderPhaseSaturated"},
            {"id": "live", "date": "2026-05-18", "status": "live", "body": "CLMM limit order settlement OrderPhaseSaturated"},
            {"id": "unrelated", "date": "2026-05-18", "status": "live", "body": "CPMM creator fee collection permissionless"},
            {"id": "wrong-clmm-page", "date": "2026-05-18", "status": "live",
             "body": "CLMM limit order settlement OrderPhaseSaturated",
             "reference_path": "reference/changelog/2026-05-18-clmm-nft-freeze.mdx"},
        ]}
        result = module.build(audit, ledger, database)
        links = result["suggestions"][0]["reference_candidates"]
        self.assertEqual({item["source_id"] for item in links}, {"planned", "live"})
        self.assertEqual({item["source_id"]: item["relationship"] for item in links},
                         {"planned": "planned_context", "live": "live_reference_check"})
        self.assertNotIn("body", str(result))


if __name__ == "__main__":
    unittest.main()
