"""Verify private review-pack provenance without printing archive text."""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("pack", type=Path)
    parser.add_argument("database", type=Path)
    args = parser.parse_args()
    pack = json.loads(args.pack.read_text(encoding="utf-8"))
    assert pack["schema_version"] == 1
    assert pack["review_status"] == "pending_human_review"
    cases = {case["conversation_family"]: case for case in pack["cases"]}
    assert len(cases) == len(pack["cases"]) == 30
    assert len(pack["queries"]) == 60
    assert len({query["id"] for query in pack["queries"]}) == 60
    counts = {family: 0 for family in cases}
    with sqlite3.connect(args.database.resolve().as_uri() + "?mode=ro", uri=True) as connection:
        for family, case in cases.items():
            assert case["approval"] is None and case["reviewer"] is None
            assert case["case_id"] == family
            assert case["split"] in {"development", "held_out"}
            expected = "held_out" if hashlib.sha256(family.encode()).digest()[0] % 3 == 0 else "development"
            assert case["split"] == expected
            for message in case["messages"]:
                row = connection.execute(
                    "SELECT m.body,m.sender,m.source_message_id,f.file_name,m.date_title FROM message_revisions m "
                    "JOIN source_files f ON f.source_file_id=m.source_file_id WHERE revision_id=?",
                    [message["revision_id"]]).fetchone()
                assert row == (message["body"], message["sender"], message["message_id"], message["file_name"], message["date"])
                assert connection.execute(
                    "SELECT 1 FROM candidate_case_messages WHERE case_id=? AND revision_id=?",
                    [case["case_id"], message["revision_id"]]).fetchone()
        for query in pack["queries"]:
            case = cases[query["conversation_family"]]
            assert query["split"] == case["split"]
            assert query["relevant"] is None and query["forbidden"] is None and query["reviewer"] is None
            message = next(m for m in case["messages"] if m["revision_id"] == query["source_revision_id"])
            assert query["query"] == message["body"]
            assert message["sender"] and message["sender"] == case["messages"][0]["sender"]
            counts[query["conversation_family"]] += 1
    assert set(counts.values()) == {2}
    print(json.dumps({"cases": 30, "queries": 60, "distinct_families": 30,
                      "source_provenance_verified": True,
                      "review_status": pack["review_status"],
                      "splits": {split: sum(c["split"] == split for c in cases.values())
                                 for split in ["development", "held_out"]}}, indent=2))


if __name__ == "__main__":
    main()
