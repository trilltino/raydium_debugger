"""Path safety and coverage checks for support attachment inventory."""

import importlib.util
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
import zipfile


SPEC = importlib.util.spec_from_file_location(
    "support_attachment_audit", Path(__file__).with_name("support-attachment-audit.py")
)
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)


class AttachmentAuditTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / "files").mkdir()
        (self.root / "files" / "answer.txt").write_text("Visible source text", encoding="utf-8")
        (self.root / "photos").mkdir()
        (self.root / "photos" / "image.jpg").write_bytes(b"not decoded")
        self.db = sqlite3.connect(":memory:")
        self.db.executescript("""
            CREATE TABLE source_files(source_file_id INT,corpus_id TEXT,file_name TEXT);
            CREATE TABLE message_revisions(revision_id INT,source_file_id INT,corpus_id TEXT,
              source_message_id TEXT,media_refs_json TEXT);
            INSERT INTO source_files VALUES(1,'support','messages.html');
            INSERT INTO source_files VALUES(2,'support','messages.html');
            INSERT INTO message_revisions VALUES
              (1,1,'support','message1','["files/old.txt"]'),
              (2,2,'support','message1','["files/answer.txt","photos/image.jpg"]'),
              (3,2,'support','message2','["../outside.txt","files/%2e%2e/secret.txt"]'),
              (4,2,'support','message3','[]');
        """)

    def tearDown(self):
        self.db.close()
        self.temp.cleanup()

    def test_path_safety(self):
        accepted = audit.safe_attachment_path(self.root, "files/answer.txt")
        self.assertEqual(accepted, (self.root / "files" / "answer.txt").resolve())
        for href in [
            "../outside.txt", "files/%2e%2e/secret.txt", "/etc/passwd",
            "https://example.com/file.txt", "files\\answer.txt", "files//answer.txt",
        ]:
            self.assertIsNone(audit.safe_attachment_path(self.root, href), href)

    def test_covers_current_refs_and_marks_media_unprocessed(self):
        report = audit.inventory(self.db, self.root)
        self.assertEqual(report["current_support_message_count"], 3)
        self.assertEqual(report["messages_with_attachments_count"], 2)
        self.assertEqual(report["attachment_reference_count"], 4)
        self.assertEqual(report["status_counts"], {"extracted": 1, "unprocessed_media": 1, "unsafe_path": 2})
        extracted = next(a for a in report["attachments"] if a["status"] == "extracted")
        self.assertEqual(extracted["revision_id"], 2)
        self.assertEqual(extracted["extracted_text"], "Visible source text")
        self.assertTrue(all(not a["extracted_text"] for a in report["attachments"] if a["status"] != "extracted"))

    def test_har_redacts_query_and_zip_is_only_listed(self):
        har = self.root / "files" / "network.har"
        har.write_text(json.dumps({"log": {"entries": [{
            "request": {"method": "GET", "url": "https://rpc.example/path?secret=token",
                        "headers": [{"value": "private-key"}]},
            "response": {"status": 429, "content": {"text": "private body"}},
        }]}}), encoding="utf-8")
        status, text = audit.extract_text(har)
        self.assertEqual(status, "extracted")
        self.assertIn("429", text)
        self.assertNotIn("secret", text)
        self.assertNotIn("private", text)
        archive = self.root / "files" / "assets.zip"
        with zipfile.ZipFile(archive, "w") as file:
            file.writestr("asset.txt", "not extracted")
        status, text = audit.extract_text(archive)
        self.assertEqual(status, "extracted")
        self.assertIn("asset.txt", text)
        self.assertNotIn("not extracted", text)

    def test_office_text_is_read_without_unpacking_files(self):
        document = self.root / "files" / "question.docx"
        with zipfile.ZipFile(document, "w") as file:
            file.writestr("word/document.xml",
                          '<w:document xmlns:w="urn:test"><w:t>Pool failed</w:t></w:document>')
        self.assertEqual(audit.extract_text(document), ("extracted", "Pool failed"))
        slides = self.root / "files" / "slides.pptx"
        with zipfile.ZipFile(slides, "w") as file:
            file.writestr("ppt/slides/slide1.xml",
                          '<p:sld xmlns:p="urn:test"><p:t>Retry after refresh</p:t></p:sld>')
        self.assertEqual(audit.extract_text(slides), ("extracted", "Retry after refresh"))


if __name__ == "__main__":
    unittest.main()
