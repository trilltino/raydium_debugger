"""Focused tests for private AI support packet and citation boundaries."""

from __future__ import annotations

import importlib.util
import hashlib
import json
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest
from unittest.mock import patch

MODULE_PATH = Path(__file__).with_name("classify-support-cases.py")
spec = importlib.util.spec_from_file_location("classify_support_cases", MODULE_PATH)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def fixture_packet():
    evidence = {
        "case_id": "case-1", "kind": "case",
        "messages": [
            {"revision_id": 10, "source_message_id": "10", "sender": "reporter",
             "body": "Swap failed again", "attachments": []},
            {"revision_id": 11, "source_message_id": "11", "sender": "reporter",
             "body": "It worked after I updated the wallet", "attachments": []},
        ],
        "references": [{"id": "repo:one", "text": "Wallet upgrade advice"}],
        "upgrade_context": [],
    }
    return {**evidence, "evidence_fingerprint": module.stable_hash(evidence)}


def fixture_answer():
    return {"outcome": "historical_resolution", "tier": "reporter_confirmed",
            "record_type": "technical_problem",
            "category": "wallet swap", "diagnosis": None,
            "resolution": "Reporter updated the wallet and then the swap worked.",
            "general_guidance": None, "confidence": "medium",
            "message_evidence": [{"revision_id": 10, "quote": "Swap failed again"},
                                 {"revision_id": 11,
                                  "quote": "It worked after I updated the wallet"}],
            "reference_ids": [], "upgrade_ids": [],
            "unanswered_questions": ["Which wallet version?"]}


class ClassificationTests(unittest.TestCase):
    def test_valid_member_quote_can_support_history(self):
        validated = module.validate_result(fixture_packet(), fixture_answer())
        self.assertEqual(validated["status"], "ai_draft_unreviewed")

    def test_unrelated_message_and_fabricated_quote_rejected(self):
        answer = fixture_answer()
        answer["message_evidence"] = [{"revision_id": 99, "quote": "It worked"}]
        with self.assertRaisesRegex(ValueError, "member revision"):
            module.validate_result(fixture_packet(), answer)
        answer["message_evidence"] = [{"revision_id": 11, "quote": "team patched it"}]
        with self.assertRaisesRegex(ValueError, "verbatim"):
            module.validate_result(fixture_packet(), answer)

    def test_reference_alone_cannot_establish_fix(self):
        answer = fixture_answer()
        answer["message_evidence"] = []
        answer["reference_ids"] = ["repo:one"]
        with self.assertRaisesRegex(ValueError, "message evidence"):
            module.validate_result(fixture_packet(), answer)
        answer["outcome"] = "general_guidance"
        answer["tier"] = "unknown"
        answer["resolution"] = "Fixed historically"
        answer["general_guidance"] = "Upgrade your wallet."
        with self.assertRaisesRegex(ValueError, "cannot claim"):
            module.validate_result(fixture_packet(), answer)

    def test_orphan_cannot_claim_historical_resolution(self):
        packet = fixture_packet()
        packet["kind"] = "orphan"
        with self.assertRaisesRegex(ValueError, "orphan"):
            module.validate_result(packet, fixture_answer())

    def test_media_only_initial_issue_can_be_cited_as_ocr(self):
        packet = fixture_packet()
        packet["messages"][0]["body"] = ""
        packet["messages"][0]["attachments"] = [{"status": "ocr_text", "text": "RPC CONNECTION error"}]
        answer = fixture_answer()
        answer["message_evidence"][0]["quote"] = "RPC CONNECTION error"
        result = module.validate_result(packet, answer)
        self.assertEqual(result["attachment_quote_revision_ids"], [10])

    def test_support_relay_can_cite_user_confirmation(self):
        packet = fixture_packet()
        packet["messages"][1]["sender"] = "support relay"
        packet["messages"][1]["body"] = "Users confirmed the fix"
        answer = fixture_answer()
        answer["message_evidence"][1]["quote"] = "Users confirmed the fix"
        self.assertEqual(module.validate_result(packet, answer)["tier"], "reporter_confirmed")
        packet["messages"][1]["body"] = "user has just replied that he was able to close!"
        answer["message_evidence"][1]["quote"] = packet["messages"][1]["body"]
        self.assertEqual(module.validate_result(packet, answer)["tier"], "reporter_confirmed")

    def test_guidance_needs_relevant_reference_and_upgrade_ids_are_bound(self):
        packet = fixture_packet()
        answer = fixture_answer()
        answer.update({"outcome": "general_guidance", "tier": "unknown",
                       "resolution": None, "general_guidance": "Upgrade your wallet before swapping.",
                       "reference_ids": ["repo:one"]})
        self.assertEqual(module.validate_result(packet, answer)["outcome"], "general_guidance")
        answer["general_guidance"] = "Compute rent for position accounts"
        with self.assertRaisesRegex(ValueError, "substantive overlap"):
            module.validate_result(packet, answer)
        answer["general_guidance"] = "Upgrade your wallet before swapping."
        answer["upgrade_ids"] = ["unrelated-announcement"]
        with self.assertRaisesRegex(ValueError, "upgrade IDs"):
            module.validate_result(packet, answer)

    def test_stale_fingerprint_is_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            packet = fixture_packet()
            module.write_jsonl(root / "packets.jsonl", [packet])
            result = module.validate_result(packet, fixture_answer())
            result["evidence_fingerprint"] = "old"
            module.write_jsonl(root / "drafts.jsonl", [result])
            summary = module.report(root / "packets.jsonl", root / "drafts.jsonl")
            self.assertEqual(summary["stale_result_count"], 1)

    def test_old_classifier_version_is_stale(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            packet = fixture_packet()
            module.write_jsonl(root / "packets.jsonl", [packet])
            result = module.validate_result(packet, fixture_answer())
            result["classification_version"] = "old-instructions"
            module.write_jsonl(root / "drafts.jsonl", [result])
            self.assertEqual(module.report(root / "packets.jsonl", root / "drafts.jsonl")["stale_result_count"], 1)

    def test_small_classification_run_preserves_other_drafts_and_reuses(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            packet = fixture_packet()
            other = {**packet, "case_id": "case-2"}
            module.write_jsonl(root / "packets.jsonl", [packet, other])
            old = {**module.validate_result(packet, fixture_answer()),
                   "case_id": "case-2", "evidence_fingerprint": other["evidence_fingerprint"]}
            module.write_jsonl(root / "drafts.jsonl", [old])
            adapter = root / "adapter.py"
            adapter.write_text("import json,sys\n"
                               "request=json.load(sys.stdin)\n"
                               f"print(json.dumps({fixture_answer()!r}))\n",
                               encoding="utf-8")
            command = json.dumps([sys.executable, str(adapter)])
            first = module.classify(root / "packets.jsonl", root / "drafts.jsonl",
                                    command, case_id="case-1")
            self.assertEqual(first["attempted"], 1)
            self.assertEqual(first["stored"], 2)
            self.assertEqual({r["case_id"] for r in module.read_jsonl(root / "drafts.jsonl")},
                             {"case-1", "case-2"})
            adapter.unlink()
            second = module.classify(root / "packets.jsonl", root / "drafts.jsonl",
                                     command, case_id="case-1")
            self.assertEqual(second["counts"], {"reused": 1})

    def test_checkpoint_survives_interrupted_batch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            packets = [{**fixture_packet(), "case_id": f"case-{number}"}
                       for number in (1, 2)]
            module.write_jsonl(root / "packets.jsonl", packets)
            calls = 0

            def interrupt_second(*_args, **_kwargs):
                nonlocal calls
                calls += 1
                if calls == 2:
                    raise KeyboardInterrupt()
                class Result:
                    returncode = 0
                    stdout = json.dumps(fixture_answer())
                    stderr = ""
                return Result()

            with patch.object(module.subprocess, "run", side_effect=interrupt_second):
                with self.assertRaises(KeyboardInterrupt):
                    module.classify(root / "packets.jsonl", root / "drafts.jsonl",
                                    "adapter", checkpoint_every=25)
            saved = list(module.read_jsonl(root / "drafts.jsonl"))
            self.assertEqual([item["case_id"] for item in saved], ["case-1"])

    def test_prepare_includes_current_case_and_orphan_with_bound_ocr(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            db = root / "case.sqlite"
            con = sqlite3.connect(db)
            con.executescript("""
                CREATE TABLE candidate_cases(case_id TEXT, is_current INTEGER, corpus_id TEXT);
                CREATE TABLE candidate_case_messages(case_id TEXT, revision_id INTEGER);
                CREATE TABLE source_files(source_file_id INTEGER, corpus_id TEXT, file_name TEXT);
                CREATE TABLE message_revisions(revision_id INTEGER, source_message_id TEXT,
                  sender TEXT, date_title TEXT, body TEXT, media_refs_json TEXT,
                  source_file_id INTEGER, corpus_id TEXT);
                INSERT INTO source_files VALUES(1,'support','messages.html');
                INSERT INTO candidate_cases VALUES('case-1',1,'support');
                INSERT INTO candidate_case_messages VALUES('case-1',10);
                INSERT INTO message_revisions VALUES
                  (10,'10','reporter','2024-01-01','Swap failed','["a.png"]',1,'support'),
                  (12,'12','other','2024-01-01','How to swap?','["c.pdf"]',1,'support');
            """)
            con.close()
            (root / "a.png").write_bytes(b"image")
            (root / "c.pdf").write_bytes(b"document")
            ocr = root / "ocr.jsonl"
            module.write_jsonl(ocr, [
                {"revision_id": 10, "source_message_id": "10", "relative_path": "a.png",
                 "sha256": hashlib.sha256(b"image").hexdigest(), "status": "ocr_text", "text": "Swap error"},
                {"revision_id": 10, "source_message_id": "wrong", "relative_path": "b.png",
                 "sha256": "def", "status": "ocr_text", "text": "Unrelated"},
                {"revision_id": 12, "source_message_id": "12", "relative_path": "c.pdf",
                 "sha256": hashlib.sha256(b"document").hexdigest(), "status": "pdf_ocr_text", "text": "Manual"},
            ])
            with patch.object(module.references, "load_index", return_value=[]), \
                 patch.object(module.references, "query", return_value=[]):
                result = module.prepare(db, root / "index", ocr, root / "packets.jsonl",
                                        include_orphans=True, export_root=root,
                                        documents_path=root / "missing.json")
            packets = list(module.read_jsonl(root / "packets.jsonl"))
            self.assertEqual(result["packets"], 2)
            self.assertEqual(packets[0]["messages"][0]["attachments"][0]["text"], "Swap error")
            self.assertEqual(len(packets[0]["messages"][0]["attachments"]), 1)
            self.assertEqual(packets[1]["kind"], "orphan")
            self.assertEqual(packets[1]["messages"][0]["attachments"][0]["status"], "pdf_ocr_text")

    def test_document_extraction_must_match_current_archive_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "note.txt").write_text("Actual text", encoding="utf-8")
            inventory = root / "documents.json"
            item = {"revision_id": 5, "source_message_id": "5",
                    "source_file": "messages.html", "relative_path": "note.txt",
                    "status": "extracted", "extracted_text": "Actual text"}
            inventory.write_text(json.dumps({"attachments": [item]}), encoding="utf-8")
            result = module.load_documents(inventory, root)
            self.assertEqual(result[5][0]["text"], "Actual text")
            item["extracted_text"] = "Stale text"
            inventory.write_text(json.dumps({"attachments": [item]}), encoding="utf-8")
            self.assertEqual(module.load_documents(inventory, root), {})


if __name__ == "__main__":
    unittest.main()
