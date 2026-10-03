"""Test-only oracle drift gates and successful Draft4 matrix coverage."""
import importlib.metadata
import pathlib
import tempfile
import unittest
from unittest.mock import patch

import jsonschema
from jsonschema.validators import validator_for

import output_schema_audit as audit
import output_schema_dialect_audit as dialect_audit
from output_schema_dialect_audit import cases


class OracleEnvironmentTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue(callable(getattr(audit, "assert_oracle_environment", None)),
                        "Both audit entry points need a shared oracle gate")
        self.version = importlib.metadata.version

    def test_installed_oracle_matches_recorded_metadata(self):
        versions, environment = audit.assert_oracle_environment()
        self.assertEqual(versions, {"jsonschema": "4.26.0", "referencing": "0.37.0", "mcp": "1.30.0"})
        self.assertNotIn("uri", environment["schema_format_checkers"])
        self.assertIsNone(environment["optional_packages"]["rfc3987"])

    def test_dependency_and_optional_package_drift_rejected(self):
        for package in ["jsonschema", "referencing", "mcp", "jsonschema-specifications", "idna", "rfc3987"]:
            with self.subTest(package=package):
                def changed_version(name):
                    return "drift" if name == package else self.version(name)
                with patch.object(importlib.metadata, "version", changed_version):
                    with self.assertRaises(AssertionError):
                        audit.assert_oracle_environment()

    def test_format_checker_addition_and_removal_rejected(self):
        checkers = jsonschema.Draft202012Validator.FORMAT_CHECKER.checkers
        for changed in [dict(checkers, uri=(lambda value: True, ValueError)),
                        {name: checker for name, checker in checkers.items() if name != "date"}]:
            with self.subTest(checkers=sorted(changed)):
                with patch.object(jsonschema.Draft202012Validator.FORMAT_CHECKER, "checkers", changed):
                    with self.assertRaises(AssertionError):
                        audit.assert_oracle_environment()

    def test_both_entry_points_reject_drift_before_capturing(self):
        def changed_version(name):
            return "drift" if name == "mcp" else self.version(name)
        with tempfile.TemporaryDirectory() as directory:
            out = pathlib.Path(directory) / "must-not-be-written.json"
            fixtures = pathlib.Path(directory) / "fixtures-must-not-be-written.json"
            for module in [audit, dialect_audit]:
                argv = [module.__file__, "--binary", "/unavailable-worker", "--out", str(out)]
                if module is dialect_audit:
                    argv.extend(["--fixtures", str(fixtures)])
                with self.subTest(entry_point=module.__name__):
                    with patch("sys.argv", argv), patch.object(importlib.metadata, "version", changed_version):
                        with self.assertRaises(AssertionError):
                            module.main()
                    self.assertFalse(out.exists())
                    self.assertFalse(fixtures.exists())


class Draft4CoverageTests(unittest.TestCase):
    def test_registered_roots_have_valid_accept_reject_pairs(self):
        rows = [(name, schema, content) for name, schema, content in cases()
                if name.startswith("draft4-valid-")]
        self.assertEqual(len(rows), 14, "Seven registered roots need two instances each")
        for index in range(0, len(rows), 2):
            accepted, rejected = rows[index:index + 2]
            self.assertEqual(accepted[1], rejected[1])
            self.assertTrue(accepted[0].endswith("-accept"))
            self.assertTrue(rejected[0].endswith("-reject"))
            for _, schema, content in [accepted, rejected]:
                self.assertIs(validator_for(schema), jsonschema.Draft4Validator)
                jsonschema.Draft4Validator.check_schema(schema)
            jsonschema.Draft4Validator(accepted[1]).validate(accepted[2])
            with self.assertRaises(jsonschema.exceptions.ValidationError):
                jsonschema.Draft4Validator(rejected[1]).validate(rejected[2])


if __name__ == "__main__":
    unittest.main()
