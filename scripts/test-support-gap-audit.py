"""Focused regression checks for the private support gap inventory."""

import importlib.util
import sqlite3
import unittest
from pathlib import Path


SPEC = importlib.util.spec_from_file_location(
    "support_gap_audit", Path(__file__).with_name("support-gap-audit.py")
)
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)


class SupportGapAuditTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.executescript("""
            CREATE TABLE candidate_cases(case_id TEXT,corpus_id TEXT,review_status TEXT,is_current INT);
            CREATE TABLE candidate_case_messages(case_id TEXT,revision_id INT);
            CREATE TABLE source_files(source_file_id INT,corpus_id TEXT,file_name TEXT);
            CREATE TABLE message_revisions(revision_id INT,source_file_id INT,corpus_id TEXT,
              source_message_id TEXT,sender TEXT,body TEXT,media_refs_json TEXT);
            CREATE TABLE case_resolution_scans(case_id TEXT,fingerprint TEXT,rule_version INT);
            CREATE TABLE case_resolution_suggestions(case_id TEXT,revision_id INT,signal TEXT);
            CREATE TABLE case_resolution_reviews(case_id TEXT,outcome TEXT,fingerprint TEXT);
            INSERT INTO source_files VALUES(1,'support','messages.html');
            INSERT INTO candidate_cases VALUES('a','support','candidate',1);
            INSERT INTO candidate_cases VALUES('b','support','candidate',1);
            INSERT INTO candidate_case_messages VALUES('a',1),('a',2),('b',3);
            INSERT INTO message_revisions VALUES
              (1,1,'support','message1','Alice','Swap fails with error','[]'),
              (2,1,'support','message2','Bob','Try changing amount','[]'),
              (3,1,'support','message3','Cara','CLMM issue?','[]'),
              (4,1,'support','message4','Dan','How do I open a pool?','[]'),
              (5,1,'support','message5','Eve','', '["photo.jpg"]');
            INSERT INTO case_resolution_suggestions VALUES('a',2,'proposal');
        """)

    def tearDown(self):
        self.db.close()

    def test_all_current_messages_covered_without_inventing_fix(self):
        report = audit.build_report(self.db)
        self.assertEqual(report["case_count"], 2)
        self.assertEqual(report["current_support_message_count"], 5)
        self.assertEqual(report["messages_in_current_cases_count"], 3)
        self.assertEqual(report["orphan_message_count"], 2)
        self.assertEqual(report["empty_body_with_media_revision_ids"], [5])
        self.assertEqual({r["message_revision_id"] for r in report["orphan_messages"]}, {4, 5})
        self.assertEqual(report["orphan_needs_followup_count"], 1)
        self.assertTrue(all(r["accepted_outcome"] is None for r in report["cases"]))
        self.assertTrue(all(r["needs_followup"] for r in report["cases"]))

    def test_proposal_and_stale_review_do_not_become_resolution(self):
        self.db.row_factory = sqlite3.Row
        messages = self.db.execute(
            "SELECT revision_id,source_message_id,sender,body FROM message_revisions "
            "WHERE revision_id IN (1,2) ORDER BY revision_id"
        ).fetchall()
        fingerprint = audit.fingerprint(messages)
        self.db.execute("INSERT INTO case_resolution_scans VALUES('a',?,3)", (fingerprint,))
        self.db.execute("INSERT INTO case_resolution_reviews VALUES('a','confirmed','stale')")
        report = audit.build_report(self.db)
        case = next(r for r in report["cases"] if r["case_id"] == "a")
        self.assertEqual(case["suggested_resolution_tier"], "proposed")
        self.assertEqual(case["resolution_review_state"], "stale_review")
        self.assertIsNone(case["accepted_outcome"])
        self.assertTrue(case["needs_followup"])


if __name__ == "__main__":
    unittest.main()
