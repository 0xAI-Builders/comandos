#!/usr/bin/env python3
"""Explicit installed-oracle regression checks; outside ordinary test discovery."""
import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

from output_schema_traversal_audit import assert_oracle_environment, directed_cases, oracle, probe

STAGE_TEST = 'output_schema::preflight::tests::audit_probe'
TRAVERSAL_TEST = 'output_schema::traversal::tests::audit_probe'
AUDIT = pathlib.Path(__file__).with_name('output_schema_traversal_audit.py')
EVIDENCE = []
FROZEN_CASES = AUDIT.with_name('output_schema_traversal_cases.json')


def cases(count):
    return [{'name': f'protocol-{index}', 'schema_json': '{"type":"string"}',
             'content_json': '"valid"' if index % 2 == 0 else '{}',
             'mode': 'ExhaustErrors', 'expected_gap': None}
            for index in range(count)]


class AuditOracleRegression(unittest.TestCase):
    def test_generator_preserves_all_frozen_inputs_and_order(self):
        frozen = json.loads(FROZEN_CASES.read_text())
        expected = [{key: value for key, value in row.items() if key != 'oracle'} for row in frozen]
        generated = directed_cases()
        EVIDENCE.append({'test': 'generator-inputs', 'expected_count': len(expected), 'generated': generated})
        self.assertEqual(len(generated), 282)
        self.assertEqual(generated, expected)
        self.assertEqual(directed_cases(), generated)

    def test_real_generate_preserves_every_frozen_oracle(self):
        before = FROZEN_CASES.read_bytes()
        frozen = json.loads(before)
        with tempfile.TemporaryDirectory() as directory:
            directory = pathlib.Path(directory)
            generated_path, capture_path = directory / 'generated.json', directory / 'oracle.json'
            command = [sys.executable, str(AUDIT), '--stage-binary', STAGE_BINARY,
                       '--binary', RELEASE_BINARY, '--cases', str(generated_path),
                       '--full-cases', str(FROZEN_CASES), '--out', str(capture_path),
                       '--generate', '--capture-only']
            result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            generated = json.loads(generated_path.read_text())
            capture = json.loads(capture_path.read_text())
            EVIDENCE.append({'test': 'real-generate', 'command': command,
                             'returncode': result.returncode, 'stdout': result.stdout.decode(),
                             'stderr': result.stderr.decode(), 'generated': generated, 'capture': capture,
                             'frozen_sha256': hashlib.sha256(before).hexdigest()})
            self.assertEqual(generated, frozen)
            self.assertEqual(capture['results'], [{'name': row['name'], **row['oracle']} for row in frozen])
        self.assertEqual(FROZEN_CASES.read_bytes(), before)

    def test_actual_probe_protocols_preserve_singleton_and_final_singleton(self):
        for count in (1, 8, 9):
            rows = cases(count)
            refs = [oracle(row) for row in rows]
            for test, prefix in ((STAGE_TEST, 'SCHEMA_PREFLIGHT='),
                                 (TRAVERSAL_TEST, 'SCHEMA_TRAVERSAL=')):
                with self.subTest(count=count, test=test):
                    sent = [{**row, 'oracle_checked': True} for row in rows]
                    actual, measurements = probe(STAGE_BINARY, sent, test, prefix)
                    EVIDENCE.append({'count': count, 'test': test, 'inputs': sent,
                                     'actual': actual, 'measurements': measurements})
                    self.assertTrue(all(m['infrastructure'] is None for m in measurements), actual)
                    self.assertEqual([row['name'] for row in actual], [row['name'] for row in rows])
                    if test == STAGE_TEST:
                        self.assertEqual([r['native'] for r in actual], ['valid'] * count)
                    else:
                        self.assertEqual([r['result'] for r in actual], [r['instance']['result'] for r in refs])

    def run_audit(self, count, fault=None, rows=None):
        inputs = cases(count) if rows is None else rows
        with tempfile.TemporaryDirectory() as directory:
            directory = pathlib.Path(directory)
            input_path, output_path = directory / 'cases.json', directory / 'result.json'
            input_path.write_text(json.dumps(inputs))
            stage = STAGE_BINARY
            if fault:
                # Exercise the real probe, then inject one transport/measurement failure.
                wrapper = directory / 'fault-probe'
                wrapper.write_text('#!' + sys.executable + '\n' +
                    'import json, subprocess, sys\n' +
                    f'fault = {fault!r}\nreal = {STAGE_BINARY!r}\n' +
                    "result = subprocess.run([real, *sys.argv[1:]], input=sys.stdin.buffer.read(), stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)\n" +
                    "prefix = 'SCHEMA_PREFLIGHT=' if fault.startswith('stage_') else 'SCHEMA_TRAVERSAL='\n" +
                    "lines = result.stdout.decode().splitlines()\n" +
                    "for index, line in enumerate(lines):\n" +
                    "    if not line.startswith(prefix): continue\n" +
                    "    if fault.endswith('missing'): lines[index] = ''; continue\n" +
                    "    if fault.endswith('malformed'): lines[index] = prefix + '{'; continue\n" +
                    "    value = json.loads(line[len(prefix):])\n" +
                    "    target = value[0] if isinstance(value, list) else value\n" +
                    "    if fault.endswith('omission'): target['result'] = 'not-run:precondition'\n" +
                    "    elif fault.endswith('selector'): target['root'] = 'Draft7Validator'\n" +
                    "    elif fault.endswith('name'): target['name'] = 'wrong-row'\n" +
                    "    elif fault.endswith('field'): target.pop('native' if isinstance(value, list) else 'result')\n" +
                    "    elif fault.endswith('root'): target.pop('root')\n" +
                    "    lines[index] = prefix + json.dumps(value)\n" +
                    "sys.stdout.write('\\n'.join(lines) + '\\n')\n" +
                    "sys.stderr.buffer.write(result.stderr)\n" +
                    "sys.exit(result.returncode)\n")
                wrapper.chmod(0o755)
                stage = str(wrapper)
            command = [sys.executable, str(AUDIT), '--stage-binary', stage,
                       '--binary', RELEASE_BINARY, '--cases', str(input_path),
                       '--full-cases', str(input_path), '--out', str(output_path)]
            result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60)
            self.assertTrue(output_path.exists(), (result.returncode, result.stderr.decode()))
            artifact = json.loads(output_path.read_text())
            EVIDENCE.append({'count': count, 'fault': fault, 'inputs': inputs, 'command': command,
                             'wrapper': wrapper.read_text() if fault else None,
                             'returncode': result.returncode, 'stdout': result.stdout.decode(),
                             'stderr': result.stderr.decode(), 'artifact': artifact})
            return result, artifact

    def test_complete_actual_audits_pass_for_one_eight_and_nine_rows(self):
        for count in (1, 8, 9):
            with self.subTest(count=count):
                result, artifact = self.run_audit(count)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                self.assertTrue(artifact['summary']['supported_cohort_pass'], artifact['summary'])
                self.assertEqual(artifact['summary']['infrastructure_failures'], 0)
                self.assertEqual(artifact['summary']['semantic_results']['Valid'], (count + 1) // 2)
                self.assertEqual(artifact['summary']['semantic_results']['Invalid'], count // 2)

    def test_incomplete_or_malformed_measurements_fail_summary_and_exit(self):
        for fault in ('traversal_missing', 'traversal_malformed', 'traversal_name',
                      'traversal_field', 'traversal_root', 'traversal_omission', 'traversal_selector',
                      'stage_missing', 'stage_malformed', 'stage_name', 'stage_field'):
            with self.subTest(fault=fault):
                result, artifact = self.run_audit(8, fault)
                self.assertEqual(result.returncode, 1, result.stderr.decode())
                summary = artifact['summary']
                self.assertFalse(summary['supported_cohort_pass'], summary)
                if fault.endswith('omission'):
                    self.assertEqual(summary['unexpected_precondition_omissions'], 8)
                elif fault.endswith('selector'):
                    self.assertEqual(summary['selector_mismatch'], 8)
                else:
                    field = 'stage_infrastructure_failures' if fault.startswith('stage_') else 'infrastructure_failures'
                    self.assertEqual(summary[field], 8)

    def test_declared_preconditions_stay_explicit_but_stage_failure_cannot_pass(self):
        rows = [{**row, 'schema_json': '{"type":7}'} for row in cases(8)]
        result, artifact = self.run_audit(8, rows=rows)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertTrue(artifact['summary']['supported_cohort_pass'])
        self.assertEqual(artifact['summary']['precondition_omissions'], 8)
        self.assertEqual(artifact['summary']['category_mismatch'], 0)
        result, artifact = self.run_audit(8, 'stage_missing', rows)
        self.assertEqual(result.returncode, 1, result.stderr.decode())
        self.assertFalse(artifact['summary']['supported_cohort_pass'])
        self.assertEqual(artifact['summary']['stage_infrastructure_failures'], 8)
        self.assertEqual(artifact['summary']['preflight_mismatch'], 0)
        self.assertEqual(artifact['summary']['unexpected_precondition_omissions'], 0)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--stage-binary', required=True)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--out', help='retain exact protocol and fault-injection evidence')
    args = parser.parse_args()
    STAGE_BINARY, RELEASE_BINARY = args.stage_binary, args.binary
    assert_oracle_environment()
    program = unittest.main(argv=[sys.argv[0]], verbosity=2, exit=False)
    if args.out:
        pathlib.Path(args.out).write_text(json.dumps(EVIDENCE, indent=2) + '\n')
    raise SystemExit(not program.result.wasSuccessful())
