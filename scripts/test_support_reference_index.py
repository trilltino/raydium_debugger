"""Small provenance and retrieval checks for the pinned repository index."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location("support_reference_index", Path(__file__).with_name("support_reference_index.py"))
module = importlib.util.module_from_spec(SPEC)
assert SPEC and SPEC.loader
SPEC.loader.exec_module(module)


class ReferenceIndexTests(unittest.TestCase):
    def _commit(self, repo: Path) -> str:
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        subprocess.run(["git", "-C", str(repo), "-c", "user.name=Index Test",
                        "-c", "user.email=index@example.invalid", "add", "."], check=True)
        subprocess.run(["git", "-C", str(repo), "-c", "user.name=Index Test",
                        "-c", "user.email=index@example.invalid", "commit", "-qm", "fixture"], check=True)
        return subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()

    def test_pinned_paths_lines_and_authority(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "refernce"
            official = root / "raydium-docs"
            community = root / "community"
            official.mkdir(parents=True)
            community.mkdir()
            (official / "withdraw.mdx").write_text("# CPMM liquidity withdraw\nReconnect wallet and retry withdrawal.\n", encoding="utf-8")
            (community / "withdraw.md").write_text("# CPMM liquidity withdraw\nTry a different wallet.\n", encoding="utf-8")
            (official / "AGENTS.md").write_text("secret prompt instruction", encoding="utf-8")
            (official / "large.ts").write_text("x" * 512_001, encoding="utf-8")
            official_commit = self._commit(official)
            community_commit = self._commit(community)
            manifest = {"repositories": [
                {"directory": "raydium-docs", "remote": "https://github.com/raydium-io/raydium-docs.git", "commit": official_commit},
                {"directory": "community", "remote": "https://github.com/other/project.git", "commit": community_commit},
            ]}
            (root / "sources.json").write_text(json.dumps(manifest), encoding="utf-8")
            output = Path(temporary) / "index.jsonl"
            first = module.build_index(root, output)
            second = module.build_index(root, output)
            self.assertEqual(first["fingerprint"], second["fingerprint"])
            self.assertEqual(first["chunks"], 2)
            index = module.load_index(output)
            matches = module.query(index, "CPMM liquidity withdraw wallet", limit=2)
            self.assertEqual(len(matches), 2)
            self.assertEqual(matches[0]["authority"], "official")
            self.assertEqual(matches[0]["start_line"], 1)
            self.assertEqual(matches[0]["end_line"], 2)
            self.assertEqual(matches[0]["url"], f"https://github.com/raydium-io/raydium-docs/blob/{official_commit}/withdraw.mdx#L1-L2")
            self.assertEqual(module.query(index, "unmatchedquartzphrase"), [])

    def test_unverified_head_and_mismatched_commit_fail(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            repo = root / "docs"
            repo.mkdir()
            (repo / "page.md").write_text("CPMM liquidity", encoding="utf-8")
            manifest_path = root / "sources.json"
            source = {"directory": "docs", "remote": "https://github.com/raydium-io/docs.git",
                      "commit": "0" * 40}
            manifest_path.write_text(json.dumps({"repositories": [source]}), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "Cannot verify pinned Git HEAD"):
                module.build_index(root, root / "index.jsonl")
            actual_commit = self._commit(repo)
            with self.assertRaisesRegex(ValueError, "Pinned commit mismatch"):
                module.build_index(root, root / "index.jsonl")
            source["commit"] = actual_commit
            manifest_path.write_text(json.dumps({"repositories": [source]}), encoding="utf-8")
            self.assertEqual(module.build_index(root, root / "index.jsonl")["files"], 1)

    def test_upgrade_announcements_keep_status_and_case_chronology(self):
        with tempfile.TemporaryDirectory() as temporary:
            ledger = Path(temporary) / "updates.json"
            ledger.write_text(json.dumps({"entries": [
                {"id": "planned", "date": "2026-05-08", "status": "planned",
                 "summary": "CLMM dynamic fee upgrade announced", "body": "CLMM dynamic fee upgrade planned",
                 "source_url": "https://t.me/example/1"},
                {"id": "live", "date": "2026-05-18", "status": "live",
                 "summary": "CLMM dynamic fee upgrade live", "body": "CLMM dynamic fee upgrade live",
                 "source_url": "https://t.me/example/2"},
            ]}), encoding="utf-8")
            matches = module.query_upgrades(ledger, "CLMM dynamic fee", "2026-05-12")
            by_id = {entry["id"]: entry for entry in matches}
            self.assertEqual(by_id["planned"]["chronology"], "planned_announcement_only")
            self.assertEqual(by_id["live"]["chronology"], "announced_after_case")
            self.assertTrue(all(entry["evidence_role"] == "dated_product_context_not_case_outcome" for entry in matches))
            telegram_matches = module.query_upgrades(ledger, "CLMM dynamic fee", "12.05.2026 09:30")
            telegram_by_id = {entry["id"]: entry for entry in telegram_matches}
            self.assertEqual(telegram_by_id["planned"]["chronology"], "planned_announcement_only")
            self.assertEqual(telegram_by_id["live"]["chronology"], "announced_after_case")

    def test_unrelated_references_and_later_upgrades_are_omitted(self):
        rows = [
            {"id": "generic", "path": "ray/white-paper.mdx", "text": "Users withdraw assets from pools after a wallet transaction", "repo": "raydium-docs-v1", "kind": "docs", "authority": "official"},
            {"id": "perps", "path": "products/perps/withdraw.mdx", "text": "Perps collateral withdrawal can fail when the timestamp expires", "repo": "raydium-docs-v1", "kind": "docs", "authority": "official"},
        ]
        found = module.query(module.ReferenceIndex(rows), "perps withdraw timestamp expired wallet", 8)
        self.assertEqual([row["id"] for row in found], ["perps"])
        with tempfile.TemporaryDirectory() as temporary:
            ledger = Path(temporary) / "updates.json"
            ledger.write_text(json.dumps({"entries": [
                {"id": "future", "date": "2026-09-09", "status": "live",
                 "summary": "Perps withdrawal upgrade", "body": "Perps withdrawal upgrade",
                 "reference_path": "products/perps/withdraw.mdx"},
            ]}), encoding="utf-8")
            self.assertEqual(module.query_upgrades(ledger, "perps withdraw timestamp", "01.09.2022"), [])

    def test_pool_topic_outweighs_conversational_words(self):
        rows = [
            {"id": "unrelated", "path": "how-creator-fees-work.mdx", "text": "A creator can work with a wallet and a pool", "repo": "raydium-docs-v1", "kind": "docs", "authority": "official"},
            {"id": "pool", "path": "user-flows/create-pool.mdx", "text": "A newly created pool can be checked after creation", "repo": "raydium-docs-v1", "kind": "docs", "authority": "official"},
        ]
        found = module.query(module.ReferenceIndex(rows), "newly created pool missing; user confirmed it works after refresh", 8)
        self.assertEqual([row["id"] for row in found], ["pool"])


if __name__ == "__main__":
    unittest.main()
