"""Visual notes must point to an inventory attachment inside the export."""

import importlib.util
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "record_media_note", Path(__file__).with_name("record-media-note.py")
)
notes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notes)


class VisualNoteTests(unittest.TestCase):
    def test_exact_source_and_hash_are_recorded(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "photo.jpg").write_bytes(b"sample")
            inventory = {"attachments": [{"revision_id": 7,
                         "source_message_id": "message3", "relative_path": "photo.jpg"}]}
            record = notes.add_note(inventory, root, root / "notes.jsonl", 7,
                                    "photo.jpg", "The screenshot shows an RPC error.", "tester")
            self.assertEqual(record["source_message_id"], "message3")
            self.assertEqual(len(record["sha256"]), 64)
            with self.assertRaises(ValueError):
                notes.add_note(inventory, root, root / "notes.jsonl", 8,
                               "photo.jpg", "An unrelated interpretation.", "tester")


if __name__ == "__main__":
    unittest.main()
