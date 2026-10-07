"""Focused tests for private OCR provenance and resumption."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "interpret_support_media", Path(__file__).with_name("interpret-support-media.py")
)
media = importlib.util.module_from_spec(spec)
spec.loader.exec_module(media)


class FakeResult:
    txts = ("Error 0x1771", "Pool account")
    scores = (0.9, 0.8)


class MediaInterpretationTests(unittest.TestCase):
    def test_provenance_text_and_resume(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "photos").mkdir()
            (root / "photos" / "screen.png").write_bytes(b"image bytes")
            attachment = {
                "revision_id": 42, "source_message_id": "message9",
                "source_file": "messages.html", "relative_path": "photos/screen.png",
                "extension": ".png", "status": "unprocessed_media",
            }
            inventory = {"report_kind": "private_support_attachment_inventory",
                         "attachments": [attachment]}
            output = root / "ocr.jsonl"
            first = media.run(inventory, root, output, lambda _: FakeResult())
            second = media.run(inventory, root, output, lambda _: FakeResult())
            self.assertEqual(first["new_records"], 1)
            self.assertEqual(second["new_records"], 0)
            record = json.loads(output.read_text(encoding="utf-8").strip())
            self.assertEqual(record["key"], "42:photos/screen.png")
            self.assertEqual(record["status"], "ocr_text")
            self.assertEqual(record["mean_confidence"], 0.85)
            self.assertTrue(record["visual_review_needed"])

    def test_escape_is_never_read(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            attachment = {"revision_id": 1, "source_message_id": "message1",
                          "source_file": "messages.html", "relative_path": "../secret.png"}
            self.assertIsNone(media.source_path(root, attachment))
            result = media.interpret(attachment, root, lambda _: self.fail("OCR called"))
            self.assertEqual(result["status"], "missing_or_unsafe")


if __name__ == "__main__":
    unittest.main()
