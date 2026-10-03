#!/usr/bin/env python3
"""Exact check_schema acceptance audit. Real gaps keep exit status 1.

Run with the frozen SDK oracle runner and the explicit Rust library test binary.
The ignored probe is test-only and receives original raw-number JSON strings.
"""
import argparse
import json
import pathlib
import subprocess
import resource
import warnings
import hashlib
import importlib.metadata
import jsonschema_specifications
import jsonschema
from output_schema_audit import assert_oracle_environment


def reference(schema_json):
    schema = json.loads(schema_json)
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", DeprecationWarning)
            selected = jsonschema.validators.validator_for(schema)
            selected.check_schema(schema)
        return "valid"
    except Exception as error:
        return "rejected:" + type(error).__name__


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--cases", default=str(pathlib.Path(__file__).with_name("output_schema_preflight_cases.json")))
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    versions, environment = assert_oracle_environment()
    bundled = pathlib.Path(__file__).parent.parent / "src/output_schema/metaschemas"
    provenance = json.loads((bundled / "PROVENANCE.json").read_text())
    source = pathlib.Path(jsonschema_specifications.__file__).parent / "schemas"
    assert provenance["version"] == environment["jsonschema_specifications"]
    for row in provenance["files"]:
        relative = row["source"].split("/schemas/", 1)[1]
        exact = (source / relative).read_bytes()
        assert (bundled / relative).read_bytes() == exact, relative
        assert hashlib.sha256(exact).hexdigest() == row["sha256"], relative
    notice = importlib.metadata.distribution("jsonschema-specifications").locate_file(
        "jsonschema_specifications-2025.9.1.dist-info/licenses/COPYING").read_bytes()
    assert (bundled / "COPYING").read_bytes() == notice
    assert hashlib.sha256(notice).hexdigest() == provenance["notice"]["sha256"]
    drafts = [jsonschema.Draft3Validator, jsonschema.Draft4Validator, jsonschema.Draft6Validator,
              jsonschema.Draft7Validator, jsonschema.Draft201909Validator, jsonschema.Draft202012Validator]
    inventories = {draft.__name__: sorted(draft.FORMAT_CHECKER.checkers) for draft in drafts}
    expected_inventories = {
        "Draft3Validator": ["date", "email", "idn-email", "ip-address", "ipv6", "regex", "time"],
        "Draft4Validator": ["email", "idn-email", "ipv4", "ipv6", "regex"],
        "Draft6Validator": ["email", "idn-email", "ipv4", "ipv6", "regex"],
        "Draft7Validator": ["date", "email", "idn-email", "idn-hostname", "ipv4", "ipv6", "regex"],
        "Draft201909Validator": environment["schema_format_checkers"],
        "Draft202012Validator": environment["schema_format_checkers"],
    }
    assert inventories == expected_inventories, inventories
    cases = json.loads(pathlib.Path(args.cases).read_text())
    usage_before = resource.getrusage(resource.RUSAGE_CHILDREN)
    process = subprocess.run([args.binary, "--exact", "output_schema::preflight::tests::audit_probe",
                              "--ignored", "--nocapture"], input=json.dumps(cases).encode(),
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
    usage_after = resource.getrusage(resource.RUSAGE_CHILDREN)
    assert process.returncode == 0, (process.returncode, process.stderr.decode(), process.stdout.decode())
    assert not process.stderr, process.stderr.decode()
    lines = [line for line in process.stdout.decode().splitlines() if line.startswith("SCHEMA_PREFLIGHT=")]
    assert len(lines) == 1, process.stdout.decode()
    actual = json.loads(lines[0].removeprefix("SCHEMA_PREFLIGHT="))
    assert len(actual) == len(cases)
    results = []
    for case, native in zip(cases, actual):
        assert native["name"] == case["name"]
        python = reference(case["schema_json"])
        mismatch = (python == "valid") != (native["native"] == "valid")
        row = {"name": case["name"], "schema_json": case["schema_json"], "python": python,
               "native": native["native"], "acceptance_mismatch": mismatch}
        results.append(row)
        if mismatch:
            print(json.dumps(row, separators=(",", ":")))
    artifact = {"versions": versions, "environment": environment, "format_inventories": inventories,
                "native": "jsonschema 0.58.4 schema stage, isolated test probe",
                "worker_cpu_seconds": usage_after.ru_utime + usage_after.ru_stime - usage_before.ru_utime - usage_before.ru_stime,
                "worker_peak_rss_kib": usage_after.ru_maxrss,
                "worker_address_space_bytes": 384 * 1024 * 1024,
                "worker_cpu_limits_seconds": [2, 3],
                "cases": len(results), "acceptance_mismatches": sum(row["acceptance_mismatch"] for row in results),
                "results": results}
    pathlib.Path(args.out).write_text(json.dumps(artifact, indent=2) + "\n")
    print(json.dumps({key: value for key, value in artifact.items() if key != "results"}))
    return int(bool(artifact["acceptance_mismatches"]))


if __name__ == "__main__":
    raise SystemExit(main())
