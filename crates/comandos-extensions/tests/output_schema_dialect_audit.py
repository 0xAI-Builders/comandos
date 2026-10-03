#!/usr/bin/env python3
"""Capture root lookup from installed CPython and audit every row honestly."""
import argparse
import json
import pathlib
import sys
import unicodedata
import warnings
from urllib.parse import urlsplit

from jsonschema.validators import validator_for
from output_schema_audit import native, reference


def cases():
    ids = ["http://json-schema.org/draft-03/schema",
           "http://json-schema.org/draft-04/schema",
           "http://json-schema.org/draft-06/schema",
           "http://json-schema.org/draft-07/schema",
           "https://json-schema.org/draft/2019-09/schema",
           "https://json-schema.org/draft/2020-12/schema"]
    roots = [("missing", None)]
    for uri in ids:
        for suffix in ["", "#", "?", "?#", "#other", "?x", "/", " "]:
            roots.append((uri + repr(suffix), uri + suffix))
        roots.extend([(uri + " scheme-case", uri.replace("http", "HTTP")),
                      (uri + " host-case", uri.replace("json-schema", "JSON-SCHEMA")),
                      (uri + " alternate-scheme", uri.replace("https:", "TEMP:").replace("http:", "https:").replace("TEMP:", "http:")),
                      (uri + " leading-c0", "\x00\x1f " + uri + "#"),
                      (uri + " tabs-cr-lf", uri.replace("schema", "sch\te\rma\n") + "#"),
                      (uri + " percent-path", uri.replace("/schema", "/%73chema")),
                      (uri + " dot-path", uri.replace("/schema", "/./schema"))])
    roots.extend(("other-" + repr(uri), uri) for uri in [
        "", "relative", "not a uri with spaces", "http://json-schema.org/schema#",
        "http://[broken", "http://broken]", "http://[127.0.0.1]", "http://[v1.]",
        "http://[V1.a]", "http://a[::1]", "http://[::1]tail", "http://[::1]",
        "http://[vA.future]", "http://[::1%zone]", "http://[::1%]",
        "http://[::1%a%b]", "http://[bad]@host", "http://[::ffff:192.0.2.1]",
        "http://[::ffff:192.000.2.1]", "http://[::1%é]", "http://[v1.\n]",
        None, 1, True, False, [], {}, ["http://json-schema.org/draft-07/schema#"]])
    # Exhaust the installed UCD's individual NFKC delimiter hazards, without
    # adding Unicode normalization or URI extras to that Python installation.
    hazards = [chr(c) for c in range(sys.maxunicode + 1)
               if chr(c) not in "/?#@:" and
               any(d in unicodedata.normalize("NFKC", chr(c)) for d in "/?#@:")]
    roots.extend(("nfkc-" + hex(ord(c)), "http://a" + c + "b") for c in hazards)
    for name, root in roots:
        schema = {"properties": {"a": {"prefixItems": [{"type": "integer"}], "items": False}},
                  "if": {"required": ["x"]}, "then": {"required": ["y"]}}
        if name != "missing":
            schema["$schema"] = root
        for variant, content in [("empty", {}), ("prefix", {"a": [1]}), ("conditional", {"x": 1})]:
            yield name + "-" + variant, schema, content
    for keyword in ["const", "enum", "default", "examples"]:
        literal = {"$schema": "https://example.invalid/literal"}
        for content in [literal, {"$schema": "changed"}]:
            yield "literal-" + keyword + repr(content), {"$schema": "unknown", "properties": {
                "literal": {keyword: [literal] if keyword in ["enum", "examples"] else literal}}}, {"literal": content}
    for root in ["unknown", "http://json-schema.org/draft-07/schema#"]:
        for nested in ["https://example.invalid/nested", "http://json-schema.org/draft-04/schema#"]:
            for content in [{}, {"n": 1.0}]:
                yield "nested-" + root + nested + repr(content), {"$schema": root, "$id": "urn:root", "properties": {
                    "n": {"$schema": nested, "$id": "urn:child", "type": "integer"}}}, content
    for content in [{}, {"n": 1}]:
        yield "pending-draft3-" + repr(content), {"$schema": ids[0], "properties": {
            "n": {"type": "integer", "required": True}}}, content


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--fixtures")
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 11, 15), sys.version
    assert unicodedata.unidata_version == "14.0.0", unicodedata.unidata_version
    rows = []
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", DeprecationWarning)
        for name, schema, content in cases():
            try:
                selected = validator_for(schema).__name__
                normalized = urlsplit(schema["$schema"]).geturl() if isinstance(schema.get("$schema"), str) else None
            except Exception as error:
                selected, normalized = "error:" + type(error).__name__, None
            schema_json, content_json = json.dumps(schema), json.dumps(content)
            expected, actual = reference(schema_json, content_json), native(args.binary, schema_json, content_json)
            rows.append(dict(name=name, schema_json=schema_json, content_json=content_json,
                             selected=selected, normalized=normalized, python=expected, native=actual,
                             acceptance_mismatch=(expected == "valid") != (actual == "valid"),
                             classification_difference=expected != actual))
    result = dict(python=sys.version, unicode=unicodedata.unidata_version, cases=len(rows),
                  acceptance_mismatches=sum(r["acceptance_mismatch"] for r in rows),
                  classification_differences=sum(r["classification_difference"] for r in rows), results=rows)
    pathlib.Path(args.out).write_text(json.dumps(result, indent=2) + "\n")
    if args.fixtures:
        pathlib.Path(args.fixtures).write_text(json.dumps([
            {k: r[k] for k in ["name", "schema_json", "content_json", "selected", "normalized", "python"]}
            for r in rows], indent=2) + "\n")
    print(json.dumps({k: v for k, v in result.items() if k != "results"}))
    return int(bool(result["acceptance_mismatches"]))


if __name__ == "__main__":
    raise SystemExit(main())
