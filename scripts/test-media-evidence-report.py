"""Verify OCR stays tied to the current source message and case."""

import importlib.util
import hashlib
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "media_evidence_report", Path(__file__).with_name("media-evidence-report.py")
)
reporter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reporter)


class MediaEvidenceTests(unittest.TestCase):
    def test_current_case_and_orphan_are_linked_without_promoting_ocr(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            database, ocr = root / "support.sqlite", root / "media.jsonl"
            connection = sqlite3.connect(database)
            connection.executescript("""
                CREATE TABLE source_files(source_file_id INTEGER,corpus_id TEXT,file_name TEXT);
                CREATE TABLE message_revisions(revision_id INTEGER,source_file_id INTEGER,
                    corpus_id TEXT,source_message_id TEXT,body TEXT);
                CREATE TABLE candidate_cases(case_id TEXT,is_current INTEGER,corpus_id TEXT);
                CREATE TABLE candidate_case_messages(case_id TEXT,revision_id INTEGER);
                INSERT INTO source_files VALUES(1,'support','messages.html');
                INSERT INTO message_revisions VALUES(5,1,'support','message5','Unable to swap');
                INSERT INTO message_revisions VALUES(6,1,'support','message6','');
                INSERT INTO candidate_cases VALUES('case-a',1,'support');
                INSERT INTO candidate_case_messages VALUES('case-a',5);
            """)
            connection.commit()
            connection.close()
            records = [
                {"revision_id": revision, "source_message_id": f"message{revision}",
                 "relative_path": f"photos/{revision}.jpg", "sha256": "a" * 64,
                 "status": "ocr_text", "line_count": 1, "mean_confidence": 0.9,
                 "text": "Error 123"}
                for revision in (5, 6)
            ]
            ocr.write_text("\n".join(json.dumps(record) for record in records) + "\n",
                           encoding="utf-8")
            report = reporter.build(database, ocr)
            self.assertEqual(report["case_count"], 1)
            self.assertEqual(report["orphan_message_count"], 1)
            self.assertEqual(report["image_count"], 2)
            self.assertEqual(report["media_with_empty_source_text_count"], 1)
            self.assertEqual(report["media_with_problem_hint_count"], 2)
            self.assertEqual(report["review_priority_counts"], {"high": 1, "medium": 1})
            self.assertTrue(all(item["visual_review_needed"] for item in report["evidence"]))
            self.assertEqual(next(item for item in report["evidence"]
                                  if item["message_revision_id"] == 5)["case_id"], "case-a")

    def test_changed_attachment_invalidates_ocr(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root / "photo.jpg"
            image.write_bytes(b"old")
            record = {"relative_path": "photo.jpg",
                      "sha256": hashlib.sha256(b"old").hexdigest()}
            self.assertTrue(reporter.matching_image(root, record))
            image.write_bytes(b"changed")
            self.assertFalse(reporter.matching_image(root, record))


if __name__ == "__main__":
    unittest.main()
