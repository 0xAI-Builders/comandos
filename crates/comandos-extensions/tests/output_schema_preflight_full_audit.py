#!/usr/bin/env python3
"""Audit a candidate preflight rejection followed by the existing worker.

This is test-only composition, not worker integration. Exit 1 preserves all
full acceptance gaps and any new regression against the complete current corpus.
"""
import argparse
import json
import pathlib
import subprocess
from output_schema_audit import assert_oracle_environment, reference, native


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage-binary", required=True)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    versions, environment = assert_oracle_environment()
    cases = json.loads(pathlib.Path(__file__).with_name("output_schema_preflight_full_cases.json").read_text())
    process = subprocess.run([args.stage_binary, "--exact", "output_schema::preflight::tests::audit_probe",
                              "--ignored", "--nocapture"], input=json.dumps(cases).encode(),
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
    assert process.returncode == 0, (process.returncode, process.stderr.decode(), process.stdout.decode())
    assert not process.stderr, process.stderr.decode()
    lines = [line for line in process.stdout.decode().splitlines() if line.startswith("SCHEMA_PREFLIGHT=")]
    assert len(lines) == 1, process.stdout.decode()
    stages = json.loads(lines[0].removeprefix("SCHEMA_PREFLIGHT="))
    assert len(stages) == len(cases)
    rows = []
    for case, stage in zip(cases, stages):
        assert stage["name"] == case["name"]
        expected = reference(case["schema_json"], case["content_json"])
        stock = native(args.binary, case["schema_json"], case["content_json"])
        candidate = stock if stage["native"] == "valid" else "invalid-schema"
        row = {"name": case["name"], "python": expected, "stock": stock, "stage": stage["native"],
               "candidate": candidate,
               "stock_acceptance_mismatch": (expected == "valid") != (stock == "valid"),
               "candidate_acceptance_mismatch": (expected == "valid") != (candidate == "valid"),
               "candidate_classification_difference": expected != candidate,
               "new_acceptance_regression": expected == stock == "valid" and candidate != "valid"}
        rows.append(row)
        if row["candidate_acceptance_mismatch"] or row["new_acceptance_regression"]:
            print(json.dumps(row, separators=(",", ":")))
    artifact = {"versions": versions, "environment": environment, "cases": len(rows),
                "stock_acceptance_mismatches": sum(r["stock_acceptance_mismatch"] for r in rows),
                "candidate_acceptance_mismatches": sum(r["candidate_acceptance_mismatch"] for r in rows),
                "candidate_classification_differences": sum(r["candidate_classification_difference"] for r in rows),
                "new_acceptance_regressions": sum(r["new_acceptance_regression"] for r in rows), "results": rows}
    pathlib.Path(args.out).write_text(json.dumps(artifact, indent=2) + "\n")
    print(json.dumps({key: value for key, value in artifact.items() if key != "results"}))
    return int(bool(artifact["candidate_acceptance_mismatches"] or artifact["new_acceptance_regressions"]))


if __name__ == "__main__":
    raise SystemExit(main())
