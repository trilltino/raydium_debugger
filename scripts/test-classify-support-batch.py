"""Focused safety checks for batched support classification."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "classify_support_batch", Path(__file__).with_name("classify-support-batch.py"))
module = importlib.util.module_from_spec(SPEC)
assert SPEC and SPEC.loader
SPEC.loader.exec_module(module)


def packet(identifier: str, revision: int) -> dict:
    body = "Pool creation failed; please investigate"
    evidence = {"case_id": identifier, "kind": "case",
                "messages": [{"revision_id": revision, "source_message_id": str(revision),
                              "sender": "user", "date": "2024-01-01", "body": body,
                              "attachments": []}],
                "references": [], "upgrade_context": []}
    return {**evidence, "evidence_fingerprint": module.classification.stable_hash(evidence)}


def open_answer(identifier: str, revision: int) -> dict:
    return {"case_id": identifier, "outcome": "open", "tier": "unknown",
            "record_type": "technical_problem", "category": "pool creation",
            "diagnosis": None, "resolution": None, "general_guidance": None,
            "confidence": "low", "message_evidence": [{"revision_id": revision,
                                                     "quote": "Pool creation failed"}],
            "reference_ids": [], "upgrade_ids": [], "unanswered_questions": ["What failed?"]}


class BatchTests(unittest.TestCase):
    def test_cross_case_quote_is_rejected(self):
        packets = [packet("case-a", 1), packet("case-b", 2)]
        answers = [open_answer("case-a", 2), open_answer("case-b", 2)]
        with patch.object(module.model_adapter, "run_batch", return_value=answers):
            results = module.classify_batch(packets, None)
        self.assertEqual(results[0]["status"], "invalid_output")
        self.assertEqual(results[1]["status"], "ai_draft_unreviewed")

    def test_completed_drafts_are_reused_on_restart(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            packets = [packet("case-a", 1), packet("case-b", 2)]
            packet_path, drafts_path = root / "packets.jsonl", root / "drafts.jsonl"
            module.classification.write_jsonl(packet_path, packets)
            answers = [open_answer("case-a", 1), open_answer("case-b", 2)]
            with patch.object(module.model_adapter, "run_batch", return_value=answers) as call:
                first = module.run(packet_path, drafts_path, root / "missing.sqlite", batch_size=2)
                second = module.run(packet_path, drafts_path, root / "missing.sqlite", batch_size=2)
            self.assertEqual(first["attempted"], 2)
            self.assertEqual(second["attempted"], 0)
            self.assertEqual(call.call_count, 1)
            self.assertEqual(len(list(module.classification.read_jsonl(drafts_path))), 2)

    def test_batch_schema_requires_case_identity(self):
        item = module.model_adapter.batch_schema()["properties"]["results"]["items"]
        self.assertIn("case_id", item["required"])
        self.assertFalse(item["additionalProperties"])

    def test_codex_prompt_escapes_invalid_source_unicode(self):
        request = {"system": "Return JSON", "packet": {"body": "bad \udc8f byte"}}
        with patch.object(module.model_adapter, "_run_codex", return_value={}) as call:
            module.model_adapter.run(request)
        prompt = call.call_args.args[0]
        self.assertIn("\\udc8f", prompt)
        prompt.encode("utf-8")

    def test_codex_cli_survives_missing_path_after_extension_update(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            executable = (home / ".vscode" / "extensions" /
                          "openai.chatgpt-99-win32-x64" / "bin" /
                          "windows-x86_64" / "codex.exe")
            executable.parent.mkdir(parents=True)
            executable.touch()
            with patch.dict("os.environ", {"SUPPORT_CASE_CODEX_EXECUTABLE": ""}), \
                    patch.object(module.model_adapter.shutil, "which", return_value=None), \
                    patch.object(Path, "home", return_value=home):
                self.assertEqual(module.model_adapter.codex_executable(),
                                 str(executable.resolve()))

    def test_saved_raw_answer_revalidates_without_model_call(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            item = packet("case-a", 1)
            packet_path, drafts_path = root / "packets.jsonl", root / "drafts.jsonl"
            module.classification.write_jsonl(packet_path, [item])
            raw = open_answer("case-a", 1)
            raw.pop("case_id")
            module.classification.write_jsonl(drafts_path, [{
                "case_id": "case-a", "status": "invalid_output", "raw_answer": raw,
                "evidence_fingerprint": item["evidence_fingerprint"],
                "classification_version": module.classification.CLASSIFICATION_VERSION,
            }])
            with patch.object(module.model_adapter, "run_batch") as call:
                result = module.run(packet_path, drafts_path, root / "missing.sqlite")
            self.assertEqual(result["attempted"], 0)
            self.assertEqual(call.call_count, 0)
            draft = next(module.classification.read_jsonl(drafts_path))
            self.assertEqual(draft["status"], "ai_draft_unreviewed")

    def test_mismatched_batch_identity_retries_packets_separately(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            packets = [packet("case-a", 1), packet("case-b", 2)]
            packet_path, drafts_path = root / "packets.jsonl", root / "drafts.jsonl"
            module.classification.write_jsonl(packet_path, packets)
            def answer(_system, batch, _model):
                if len(batch) > 1:
                    return [open_answer("wrong-id", 1)]
                return [open_answer(batch[0]["case_id"], batch[0]["messages"][0]["revision_id"])]
            with patch.object(module.model_adapter, "run_batch", side_effect=answer) as call:
                result = module.run(packet_path, drafts_path, root / "missing.sqlite", batch_size=2)
            self.assertEqual(result["attempted"], 2)
            self.assertEqual(call.call_count, 3)
            self.assertEqual({draft["status"] for draft in module.classification.read_jsonl(drafts_path)},
                             {"ai_draft_unreviewed"})


if __name__ == "__main__":
    unittest.main()
