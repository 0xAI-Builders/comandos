#!/usr/bin/env python3
"""Audit acceptance against MCP's exact jsonschema.validate/empty Registry call.

Exit 1 records real acceptance mismatches. Never bless native differences.
Run only through the approved read-only Python oracle sandbox.
Raw JSON strings preserve decimal/exponent spelling in the native child while
Python deliberately parses the same bytes into its native int/float types.
"""
import argparse
import importlib.metadata
import json
import pathlib
import subprocess
import warnings
import jsonschema
from referencing import Registry


def reference(schema_json, content_json):
    schema, content = json.loads(schema_json), json.loads(content_json)
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", DeprecationWarning)
            jsonschema.validate(content, schema, registry=Registry())
        return "valid"
    except jsonschema.exceptions.SchemaError:
        return "invalid-schema"
    except jsonschema.exceptions.ValidationError:
        return "invalid-content"
    except Exception as error:
        # Report the exception type only. Reference/schema/instance strings are
        # fixture artifacts, never taken from live configurations or sessions.
        return "execution-failure:" + type(error).__name__


def native(binary, schema_json, content_json):
    payload = '{"schema":' + schema_json + ',"structuredContent":' + content_json + '}'
    try:
        process = subprocess.run([binary, "__schema_worker"], input=payload.encode(),
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5)
    except subprocess.TimeoutExpired:
        return "execution-failure:Deadline"
    if process.returncode:
        return "execution-failure:" + ("Signal" + str(-process.returncode) if process.returncode < 0 else "Exit" + str(process.returncode))
    statuses = {b"valid\n": "valid", b"invalid-schema\n": "invalid-schema",
                b"invalid-content\n": "invalid-content", b"execution-failure\n": "execution-failure"}
    if process.stderr or process.stdout not in statuses:
        return "execution-failure:UnsafeOutput"
    return statuses[process.stdout]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--cases", default=str(pathlib.Path(__file__).with_name("output_schema_cases.json")))
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    versions = {name: importlib.metadata.version(name) for name in ["jsonschema", "referencing", "mcp"]}
    assert versions == {"jsonschema": "4.26.0", "referencing": "0.37.0", "mcp": "1.30.0"}, versions
    optional = {}
    for package in ["rfc3987", "rfc3986-validator", "rfc3987-syntax", "fqdn", "idna", "isoduration", "webcolors"]:
        try:
            optional[package] = importlib.metadata.version(package)
        except importlib.metadata.PackageNotFoundError:
            optional[package] = None
    environment = {"optional_packages": optional,
                   "schema_format_checkers": sorted(jsonschema.Draft202012Validator.FORMAT_CHECKER.checkers),
                   "jsonschema_specifications": importlib.metadata.version("jsonschema-specifications")}
    rows = []
    for case in json.loads(pathlib.Path(args.cases).read_text()):
        expected = reference(case["schema_json"], case["content_json"])
        actual = native(args.binary, case["schema_json"], case["content_json"])
        mismatch = (expected == "valid") != (actual == "valid")
        row = {"name": case["name"], "python": expected, "native": actual,
               "acceptance_mismatch": mismatch, "classification_difference": expected != actual}
        rows.append(row)
        if mismatch:
            print(json.dumps(row, separators=(",", ":")))
    artifact = {"versions": versions, "environment": environment, "native": "jsonschema 0.58.4, offline, instance formats disabled",
                "cases": len(rows), "acceptance_mismatches": sum(row["acceptance_mismatch"] for row in rows),
                "classification_differences": sum(row["classification_difference"] for row in rows), "results": rows}
    pathlib.Path(args.out).write_text(json.dumps(artifact, indent=2) + "\n")
    print(json.dumps({key: value for key, value in artifact.items() if key != "results"}))
    return int(bool(artifact["acceptance_mismatches"]))


if __name__ == "__main__":
    raise SystemExit(main())
