"""Optional audit protocol/resource faults, excluded from ordinary discovery."""
import argparse
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

import output_schema_regex_audit as audit

BINARY=None
EVIDENCE=[]


class ProtocolTests(unittest.TestCase):
    def test_missing_duplicate_reordered_and_malformed_rows(self):
        rows=[{'name':'a'},{'name':'b'}]
        for values in [[], [{'name':'a','result':'Match','value':True}]*2,
            [{'name':'b','result':'FormatValid'},{'name':'a','result':'FormatValid'}],
            [{'name':'a','result':'Match'},{'name':'b','result':'Match','value':True}],
            [{'name':'a','result':'invented'},{'name':'b','result':'Match','value':True}],
            [{'name':'a','result':'ReError','position':'2'},{'name':'b','result':'Match','value':True}]]:
            output='\n'.join(audit.PREFIX+json.dumps(value) for value in values)
            with self.subTest(values=values),self.assertRaises(ValueError): audit.decode_output(output,rows)

    def test_fixture_regeneration_preserves_inputs_order_and_oracles(self):
        rows=json.loads(audit.CASES.read_text())
        self.assertEqual([{k:v for k,v in r.items() if k!='oracle'} for r in rows],audit.definitions())
        for row in rows: self.assertEqual(row['oracle'],audit.reference(row),row['name'])

    def test_zero_one_eight_nine_rows_batching_and_direct_limit(self):
        if BINARY is None: self.skipTest('requires --binary')
        row=json.loads(audit.CASES.read_text())[0]
        for count in [0,1,8,9]:
            rows=[dict(row,name='fault/'+str(index)) for index in range(count)]
            values,measurements=audit.probe(BINARY,rows)
            EVIDENCE.append(dict(case='batched/'+str(count),values=values,measurements=measurements))
            self.assertEqual(len(values),count)
            self.assertTrue(all(v['result']=='Match' for v in values))
            self.assertTrue(all(m['count']<=8 and m['infrastructure'] is None for m in measurements))
            for measured in measurements:
                self.assertIn('process',measured)
                self.assertEqual(measured['process']['returncode'],0)
                self.assertEqual(measured['process']['stderr'],'')
                self.assertIn(audit.PREFIX,measured['process']['stdout'])
        for count in [0,9]:
            child=subprocess.run([BINARY,'--exact',audit.TEST,'--ignored','--nocapture'],
                input=json.dumps([row]*count).encode(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=10)
            EVIDENCE.append(dict(case='direct/'+str(count),returncode=child.returncode,
                stdout=child.stdout.decode(),stderr=child.stderr.decode()))
            self.assertEqual(child.returncode,0 if count==0 else 101)

    def test_schema_probe_rejects_missing_or_changed_exact_proof(self):
        if BINARY is None: self.skipTest('requires --binary')
        row=next(r for r in json.loads(audit.CASES.read_text()) if r['name']=='schema/no-root')
        for key,value in [('oracle_checked',False),('schema_json','{"pattern":"b"}')]:
            bad=dict(row,oracle_checked=True); bad[key]=value
            values,measurements=audit.probe(BINARY,[bad])
            EVIDENCE.append(dict(case='proof/'+key,values=values,measurements=measurements))
            self.assertTrue(values[0]['result'].startswith('infrastructure:'))
            self.assertIsNotNone(measurements[0]['infrastructure'])

    def test_child_timeout_is_reaped_and_crash_is_infrastructure(self):
        if BINARY is None: self.skipTest('requires --binary')
        row=json.loads(audit.CASES.read_text())[0]
        values,measurements=audit.probe(BINARY,[row],timeout=0.000001)
        EVIDENCE.append(dict(case='timeout',values=values,measurements=measurements))
        self.assertEqual(values[0]['result'],'infrastructure:TimeoutExpired')
        with tempfile.TemporaryDirectory() as folder:
            child=pathlib.Path(folder)/'crash'
            child.write_text('#!/bin/sh\nkill -KILL $$\n'); child.chmod(0o700)
            values,measurements=audit.probe(str(child),[row])
            EVIDENCE.append(dict(case='crash',values=values,measurements=measurements))
            self.assertEqual(values[0]['result'],'infrastructure:RuntimeError')


def main():
    global BINARY
    parser=argparse.ArgumentParser(); parser.add_argument('--binary'); parser.add_argument('--out'); args=parser.parse_args()
    BINARY=args.binary
    audit.assert_oracle_environment(); audit.verify_sources()
    result=unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(ProtocolTests))
    if BINARY: assert not result.skipped, 'explicit actual-probe faults must execute'
    if args.out:
        pathlib.Path(args.out).write_text(json.dumps(dict(tests=result.testsRun,failures=len(result.failures),
            errors=len(result.errors),skipped=len(result.skipped),actual_probe_faults=EVIDENCE),indent=2)+'\n')
    return int(not result.wasSuccessful())


if __name__=='__main__': sys.exit(main())
