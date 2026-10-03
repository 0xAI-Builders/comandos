#!/usr/bin/env python3
"""Explicit installed-oracle audit of the private Py311SreV1 literal slice."""
import argparse
import hashlib
import json
import pathlib
import re
import resource
import subprocess
import sys
import time
import unicodedata

import jsonschema
from output_schema_audit import assert_oracle_environment
from output_schema_traversal_audit import DRAFTS, verify_sources

CASES = pathlib.Path(__file__).with_name('output_schema_regex_cases.json')
TEST = 'output_schema::python_regex::tests::audit_probe'
PREFIX = 'SCHEMA_REGEX='


def definitions():
    rows = []
    def primitive(label, pattern, text='', gap=False, **extra):
        rows.append(dict(name='primitive/' + label, op='search',
            pattern=[ord(c) for c in pattern], text=[ord(c) for c in text],
            disposition='Grammar' if gap else 'implemented', **extra))
    for label, pattern, text in [
        ('empty-empty','',''), ('empty-text','','abc'), ('literal','a','ba'),
        ('short','ab','a'), ('nul','a\0b','xa\0b'), ('overlap','aab','aaab'),
        ('accent','é','xé'), ('emoji','😀','x😀'), ('surrogate','\ud800','x\ud800'),
        ('surrogate-dot','.','\udfff'), ('scalar-end','\U0010ffff','\U0010ffff'),
        ('escape-dollar',r'\$','$'), ('escape-dot',r'\.','.'), ('escape-slash',r'\\','\\'),
        ('controls',r'\a\f\n\r\t\v','\a\f\n\r\t\v'),
        ('trailing','é😀\\',''), ('unknown',r'é😀\q',''),
        ('literal-long','a'*2048,'a'*2048), ('miss-long','a'*128+'b','a'*2048)]:
        primitive(label, pattern, text)
    for pattern in ['^a$',r'\Aa\Z',r'a\Z','$','$$','^^a$$','^$','a^','a$b']:
        for index,text in enumerate(['','a','a\n','a\r\n','ba','\n','\n\n','a\nb']):
            primitive('anchor/'+pattern+'/'+str(index), pattern, text)
    for index,text in enumerate(['\n','\r','\x85','\u2028','😀','\0','']):
        primitive('dot/'+str(index), '.', text)
    for n in range(128):
        c=chr(n)
        if not c.isalnum(): primitive('punctuation/'+str(n), '\\'+c, c)
    for c in 'ceghjklopqyCEFGHIJKLMOPQRTVXY':
        primitive('bad-escape/'+c, '\\'+c)
    for index,pattern in enumerate([r'\d+', '[a]', 'a{2}', 'a*', 'a|b', '(a)',
        r'(?P<n>a)(?P=n)', '(?i)a', r'\b', r'\B', r'\x61', r'\N{SPACE}',
        '[', '(?P<', '(?<=a+)b', 'a**', r'\q[', '[\\', r'\q\d',
        r'\1',r'\0',r'\u0061',r'\U00000061',r'\é',']','}','a+','a?']):
        primitive('gap/'+str(index),pattern,'a',True)
    for label,value in [('integer',1),('null',None),('array',[]),('object',{}),
                        ('eligible',r'\q'),('literal','^a$'),('gap',r'\d+')]:
        rows.append(dict(name='format/'+label,op='format',value=value,
            disposition='Grammar' if label=='gap' else 'implemented'))
    classes = dict(zip(DRAFTS, [jsonschema.Draft4Validator,jsonschema.Draft6Validator,
        jsonschema.Draft7Validator,jsonschema.Draft201909Validator,jsonschema.Draft202012Validator]))
    for draft in classes:
        for label,pattern,text in [('lf','^a$','a\n'),('match','a','ba'),('miss','ab','a'),
            ('integer',r'\d+',1),('null',r'\d+',None),('array',r'\d+',[]),('object',r'\d+',{}),
            ('invalid',r'\q','a'),('valid-gap',r'\d+','١'),('invalid-gap','[','a'),
            ('transport','a','\ud800'),('schema-scope','a','a')]:
            schema={'$schema':DRAFTS[draft], 'pattern':pattern}
            if label=='schema-scope': schema['type']=3
            rows.append(dict(name='schema/'+draft+'/'+label,op='schema',draft=draft,
                schema_json=json.dumps(schema,separators=(',',':')),
                content_json=json.dumps(text,separators=(',',':')),
                disposition='TextTransport' if label=='transport' else
                    'Grammar' if label=='valid-gap' else 'implemented'))
        rows.append(dict(name='schema/'+draft+'/unrelated-valid',op='schema',draft=draft,
            schema_json=json.dumps({'$schema':DRAFTS[draft],'pattern':'a','type':'string'}),
            content_json='"a"',disposition='SchemaScope'))
    rows.append(dict(name='schema/no-root',op='schema',draft='draft202012',
        schema_json='{"pattern":"a"}', content_json='"a"',disposition='implemented'))
    rows.append(dict(name='joined/duplicate',op='joined',patterns=['(?P<x>a)','(?P<x>b)'],
        disposition='prerequisite'))
    for label,pattern,text,budget in [
        ('zero','a','a',{'work':0}),('pattern-cap','a'*16385,'',{}),
        ('subject-cap','a','a'*262145,{}),('memory','a','a',{'memory':0}),
        ('words','a','a',{'words':2}),('search-work','a'*256+'b','a'*16000,{}),
        ('compile-work','a','a',{'work':2})]:
        primitive('budget/'+label,pattern,text,budget=budget,protective=True)
    primitive('transport/above-max','a','a',transport=True)
    rows[-1]['pattern']=[0x110000]
    rows.append(dict(name='schema/unknown-declaration',op='schema',draft='draft202012',
        schema_json='{"$schema":"urn:unregistered","pattern":"a"}',content_json='"a"',
        disposition='SchemaScope'))
    rows.append(dict(name='schema/registered-alias',op='schema',draft='draft7',
        schema_json='{"$schema":"HTTP://json-schema.org/draft-07/schema#","pattern":"^a$"}',
        content_json='"a\\n"',disposition='implemented'))
    assert len(rows)==len({r['name'] for r in rows})
    return rows


def compile_reference(pattern):
    try:
        return re.compile(pattern), {'result':'Compiled'}
    except re.error as error:
        return None, {'result':'ReError','position':error.pos,'exception':'error'}
    except Exception as error:
        return None, {'result':'CompileAbort:'+type(error).__name__}


def reference(row):
    if row['op']=='schema':
        schema=json.loads(row['schema_json']); text=json.loads(row['content_json'])
        selected=jsonschema.validators.validator_for(schema)
        classes=dict(zip(DRAFTS,[jsonschema.Draft4Validator,jsonschema.Draft6Validator,
            jsonschema.Draft7Validator,jsonschema.Draft201909Validator,jsonschema.Draft202012Validator]))
        assert selected is classes[row['draft']]
        proof={'selected_class':selected.__name__, 'schema_sha256':hashlib.sha256(row['schema_json'].encode()).hexdigest()}
        try: selected.check_schema(schema)
        except Exception as error:
            return {'schema':{'result':'Abort:'+type(error).__name__},'proof':proof,
                'instance':{'result':'not-run:exact-schema-precondition'}}
        result='Valid' if selected(schema).is_valid(text) else 'Invalid'
        return {'schema':{'result':'valid'},'proof':proof,'instance':{'result':result}}
    if row['op']=='joined':
        singles=[compile_reference(p)[1] for p in row['patterns']]
        return {'individual':singles,'joined':compile_reference('|'.join(row['patterns']))[1]}
    if row['op']=='format':
        if not isinstance(row['value'],str): return {'result':'FormatValid'}
        _,compiled=compile_reference(row['value'])
        return {'result':'FormatValid'} if compiled['result']=='Compiled' else compiled
    if row.get('transport'): return {'result':'not-run:code-point-precondition'}
    pattern=''.join(map(chr,row['pattern'])); text=''.join(map(chr,row['text']))
    regex,compiled=compile_reference(pattern)
    return {'compile':compiled,'search':{'result':'Match','value':regex.search(text) is not None}
        if regex is not None else {'result':'not-run:compile-precondition'}}


def decode_output(output, rows):
    values=[json.loads(line[len(PREFIX):]) for line in output.splitlines() if line.startswith(PREFIX)]
    if len(values)!=len(rows): raise ValueError('missing or extra rows')
    for row,value in zip(rows,values):
        if not isinstance(value,dict) or value.get('name')!=row['name'] or not isinstance(value.get('result'),str):
            raise ValueError('malformed, reordered or duplicated row')
        result=value['result']
        allowed={'Match','ReError','FormatValid','Valid','Invalid','InternalProgram',
            'ScopeGap:Grammar','ScopeGap:Unicode14','ScopeGap:MatcherFeature','ScopeGap:TextTransport',
            'ScopeGap:SchemaScope','BudgetBoundary:Decode','BudgetBoundary:Compile','BudgetBoundary:Search',
            'CompileAbort:OverflowError','CompileAbort:ValueError','CompileAbort:RecursionError'}
        if result not in allowed: raise ValueError('unknown result category')
        if result=='Match' and not isinstance(value.get('value'),bool): raise ValueError('missing match boolean')
        if (result=='ReError' and 'position' not in value) or ('position' in value and
            value['position'] is not None and (type(value['position']) is not int or value['position']<0)):
            raise ValueError('malformed code-point position')
    return values


def probe(binary,rows,timeout=10):
    actual=[]; measures=[]
    for start in range(0,len(rows),8):
        batch=rows[start:start+8]; before=resource.getrusage(resource.RUSAGE_CHILDREN); began=time.monotonic()
        process={}
        try:
            child=subprocess.run([binary,'--exact',TEST,'--ignored','--nocapture'],
                input=json.dumps(batch).encode(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=timeout)
            process=dict(returncode=child.returncode,stdout=child.stdout.decode(),stderr=child.stderr.decode())
            if child.returncode or child.stderr: raise RuntimeError('child exit/stderr: '+str(child.returncode))
            values=decode_output(child.stdout.decode(),batch); infrastructure=None
        except (OSError,ValueError,RuntimeError,subprocess.TimeoutExpired) as error:
            infrastructure=type(error).__name__
            process['exception']=infrastructure
            process['detail']=str(error)
            if isinstance(error,subprocess.TimeoutExpired):
                process.update(timeout_seconds=timeout,stdout=(error.output or b'').decode(),stderr=(error.stderr or b'').decode())
            values=[{'name':row['name'],'result':'infrastructure:'+infrastructure} for row in batch]
        actual.extend(values); after=resource.getrusage(resource.RUSAGE_CHILDREN)
        measures.append(dict(start=start,count=len(batch),seconds=time.monotonic()-began,
            cpu_seconds=after.ru_utime+after.ru_stime-before.ru_utime-before.ru_stime,
            child_peak_rss_kib_cumulative=after.ru_maxrss,infrastructure=infrastructure,process=process))
    return actual,measures


def expected(row):
    ref=row['oracle']
    if row['op']=='joined': return {'result':'not-run:joined-prerequisite'}
    if row['op']=='schema' and ref['schema']['result']!='valid': return ref['instance']
    if row.get('transport'): return {'result':'ScopeGap:TextTransport','position':0}
    if row.get('protective'): return None
    if row['disposition']!='implemented': return {'result':'ScopeGap:'+row['disposition']}
    if row['op']=='schema': return ref['instance']
    if row['op']=='format': return {k:v for k,v in ref.items() if k!='exception'}
    operation=ref['search'] if ref['compile']['result']=='Compiled' else ref['compile']
    return {k:v for k,v in operation.items() if k!='exception'}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary'); parser.add_argument('--cases',default=str(CASES))
    parser.add_argument('--out',required=True); parser.add_argument('--capture-only',action='store_true')
    parser.add_argument('--generate',action='store_true'); args=parser.parse_args()
    versions,environment=assert_oracle_environment(); verify_sources()
    assert sys.version_info[:3]==(3,11,15) and unicodedata.unidata_version=='14.0.0'
    provenance=json.loads(pathlib.Path('/work/crates/comandos-extensions/src/output_schema/python_regex/PROVENANCE.json').read_text())
    for entry in provenance['sources']+provenance['notices']:
        if 'source' in entry:
            path=pathlib.Path(entry['source'].replace('/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration','/work'))
            assert hashlib.sha256(path.read_bytes()).hexdigest()==entry['sha256'],path
    from re import _constants as constants
    constant_names=['MAGIC','MAXREPEAT','MAXGROUPS','SUCCESS','ANY','AT','LITERAL',
        'AT_BEGINNING','AT_BEGINNING_STRING','AT_END','AT_END_STRING']
    measured_constants={name:int(getattr(constants,name)) for name in constant_names}
    assert list(measured_constants.values())==[20220615,4294967295,1073741823,1,2,6,16,0,2,5,7]
    if args.generate:
        rows=definitions()
        for row in rows: row['oracle']=reference(row)
        pathlib.Path(args.cases).write_text(json.dumps(rows,indent=2,ensure_ascii=True)+'\n')
    rows=json.loads(pathlib.Path(args.cases).read_text())
    assert [{k:v for k,v in row.items() if k!='oracle'} for row in rows]==definitions()
    for row in rows: assert row['oracle']==reference(row),row['name']
    artifact=dict(versions=versions,environment=environment,python=sys.version,unicode=unicodedata.unidata_version,
        constants=measured_constants,cases_sha256=hashlib.sha256(pathlib.Path(args.cases).read_bytes()).hexdigest())
    if args.capture_only: artifact['results']=rows
    else:
        assert args.binary
        sent=[]
        for row in rows:
            if row['op']=='joined': continue
            if row['op']=='schema' and row['oracle']['schema']['result']!='valid': continue
            sent.append({**row,'oracle_checked':row['oracle'].get('schema',{}).get('result')=='valid'})
        actual,measures=probe(args.binary,sent); indexed={r['name']:r for r in actual}; results=[]
        for row in rows:
            want=expected(row); native=indexed.get(row['name'],want)
            unresolved=native['result'].startswith(('ScopeGap:','BudgetBoundary:','not-run:','infrastructure:'))
            mismatch=(not native['result'].startswith('BudgetBoundary:')) if row.get('protective') else (
                any(native.get(k)!=v for k,v in want.items()))
            unexpected_gap=row['disposition']=='implemented' and not row.get('protective') and not row.get('transport') and native['result'].startswith('ScopeGap:')
            results.append({**row,'native':native,'expected':want,'mismatch':mismatch,'unresolved':unresolved,
                'unexpected_gap':unexpected_gap,'infrastructure_failure':native['result'].startswith('infrastructure:')})
        artifact.update(results=results,measurements=measures,
            resource_bounds=dict(address_space_bytes=384*1024*1024,cpu_seconds=[2,3],batch_size=8,deadline_seconds=10),
            summary={key:sum(r[key] for r in results) for key in ('mismatch','unresolved','unexpected_gap','infrastructure_failure')})
    pathlib.Path(args.out).write_text(json.dumps(artifact,indent=2,ensure_ascii=True)+'\n')
    print(json.dumps(artifact.get('summary',{'captured':len(rows)})))
    return int(not args.capture_only and any(artifact['summary'].values()))


if __name__=='__main__': raise SystemExit(main())
