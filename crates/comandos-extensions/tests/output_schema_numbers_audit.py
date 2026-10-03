#!/usr/bin/env python3
"""Explicit private numeric audit; parse and schema prerequisites stay visible."""
import argparse
import hashlib
import json
import pathlib
import struct
import sys
from fractions import Fraction

from jsonschema._utils import equal, uniq
from output_schema_audit import assert_oracle_environment
from output_schema_traversal_audit import DRAFTS, oracle, outcome, probe, verify_sources


class Raw(str):
    """Generator-only raw JSON token, never used as a replacement instance."""


def dump(value):
    if isinstance(value, Raw):
        return str(value)
    if isinstance(value, dict):
        return '{' + ','.join(json.dumps(k) + ':' + dump(v) for k, v in value.items()) + '}'
    if isinstance(value, list):
        return '[' + ','.join(map(dump, value)) + ']'
    return json.dumps(value, ensure_ascii=False, separators=(',', ':'))


def bits(value):
    return struct.pack('>d', value).hex()


def primitive(row):
    try:
        a = json.loads(row['a_json'])
        b = json.loads(row['b_json']) if 'b_json' in row else None
    except Exception as error:
        return {'result': 'not-run:parse-precondition', 'exception': type(error).__name__}
    try:
        op = row['op']
        if op == 'parse':
            return {'result': 'Primitive', 'kind': type(a).__name__,
                    **({'bits': bits(a)} if isinstance(a, float) else {})}
        if op == 'to_float':
            return {'result': 'Primitive', 'bits': bits(float(a))}
        if op == 'compare':
            return {'result': 'Primitive', 'comparison': -1 if a < b else 1 if a > b else 0 if a == b else None}
        if op == 'equal':
            result = equal(a, b)
        elif op == 'unique':
            result = uniq(a)
        elif isinstance(b, float):
            quotient = a / b
            try:
                result = int(quotient) == quotient
            except OverflowError:
                result = (Fraction(a) / Fraction(b)).denominator == 1
        else:
            result = not (a % b)
        return {'result': 'Primitive', 'value': result}
    except Exception as error:
        return {'result': 'Abort:' + type(error).__name__}


def reference(row):
    if 'op' in row:
        return primitive(row)
    # Both complete documents are parsed, schema first, before validator selection.
    try:
        json.loads(row['schema_json'])
        json.loads(row['content_json'])
    except Exception as error:
        return {'parse': {'result': 'Abort:' + type(error).__name__},
                'instance': {'result': 'not-run:parse-precondition'}}
    return {'parse': {'result': 'valid'}, **oracle(row)}


def directed_cases():
    rows = []
    def add(group, label, schema, content, drafts=None):
        for draft in drafts or DRAFTS:
            for mode in ('FirstError', 'ExhaustErrors'):
                rows.append({'name': f'{draft}/{group}/{label}/{mode}', 'group': group, 'draft': draft,
                    'mode': mode, 'schema_json': dump({'$schema': DRAFTS[draft], **schema}),
                    'content_json': dump(content)})
    def prim(op, label, a, b=None):
        rows.append({'name': f'primitive/{op}/{label}', 'group': 'primitive', 'op': op,
                     'a_json': dump(a), **({'b_json': dump(b)} if b is not None else {})})
    tokens = ['1','1.0','1e0','-0','-0.0','1e-400','-1e-400','5e-324','1e309','-1e309',
              'true','false','"1"','18446744073709551616','9007199254740993','9007199254740993.0',
              '-18446744073709551616','-9007199254740993','-9007199254740993.0']
    for kind in ('integer','number',['string','integer'],['integer','string'],['number','boolean']):
        for index, token in enumerate(tokens):
            add('type', f'{dump(kind)}/{index}', {'type':kind}, Raw(token))
    for index,token in enumerate(tokens):
        if token not in ('true','false','"1"'):
            prim('parse',f'kind/{index}',Raw(token))
    half = '1.00000000000000011102230246251565404236316680908203125'
    parse_tokens = [half,half[:-1]+'6','2.4703282292062327e-324','2.4703282292062328e-324',
        '4.9406564584124654e-324','2.225073858507201e-308','2.2250738585072014e-308',
        '1.7976931348623157e308','1.7976931348623159e308']
    for index, token in enumerate(parse_tokens):
        for sign in ('','-'): prim('parse', f'{index}/{sign or "positive"}', Raw(sign+token))
    for index, token in enumerate(['1e1000000000','-1e1000000000','1e-1000000000','0e1000000000','1e'+'9'*4096,'-'+'9'*4301+'.0']):
        prim('parse', f'huge/{index}', Raw(token))
        add('exponent', str(index), {'type':'number'}, Raw(token))
    ints = [2**53-1,2**53,2**53+1,2**53+3,2**100+2**46-1,2**100+2**46,2**100+2**46+1]
    threshold = 2**1024-2**970
    ints += [threshold-1,threshold,threshold+1]
    for index, integer in enumerate(ints):
        for sign in (1,-1): prim('to_float', f'{index}/{sign}', Raw(str(sign*integer)))
    bound_pairs = [('9007199254740992.0','9007199254740993'),('9007199254740993','9007199254740992.0'),
        ('18446744073709551616','18446744073709551617'),('18446744073709551617','18446744073709551616.0'),
        (str(10**400),'1e309'),('1e309',str(10**400)),('1e309','2e309')]
    for index,(bound,content) in enumerate(bound_pairs):
        for sign in ('','-'):
            prim('compare',f'{index}/{sign or "positive"}',Raw(sign+content),Raw(sign+bound))
            for key in ('minimum','maximum','exclusiveMinimum','exclusiveMaximum'):
                add('bound',f'{index}/{sign or "positive"}/{key}',{key:Raw(sign+bound)},Raw(sign+content),
                    list(DRAFTS)[1:] if key.startswith('exclusive') else None)
    for key in ('minimum','maximum'):
        for token in ('1','1.0'):
            add('legacy-exclusive',key+'/'+token,{key:1,'exclusive'+key.title():True},Raw(token),['draft4'])
        for index,content in enumerate([{},[],None,True]): add('bound-inapplicable',key+'/'+str(index),{key:Raw('1e309')},content)
    arithmetic = [('0.1','0.3'),('0.1','0.30000000000000004'),('0.1','0.5'),('0.5','1e308'),
        ('0.5','1.7976931348623157e308'),('5e-324','1.0'),('0.1','1e308'),('1e309','1'),
        ('1e309','1e309'),('2.0','1e309'),('2','1e309'),('2','-1e309'),('2','-4.0'),('2','-3.0')]
    arithmetic += [(d,str(10**400)) for d in ('2','2.0','0.5','1e309')]+[(str(10**400),'1.0')]
    for index,(divisor,instance) in enumerate(arithmetic):
        add('multiple',str(index),{'multipleOf':Raw(divisor)},Raw(instance))
        prim('multiple',str(index),Raw(instance),Raw(divisor))
    for index,(divisor,instance) in enumerate([('0','1'),('0.0','1'),('0.0',str(10**400)),('0','1.0'),('0.0','1e309')]):
        prim('multiple','zero/'+str(index),Raw(instance),Raw(divisor))
    for index,(key,param) in enumerate([('multipleOf','0'),('multipleOf','0.0'),('multipleOf','1e-400'),
        ('multipleOf','-1'),('maximum','true'),('maxLength','1.5'),('maxItems','1e309')]):
        add('schema-invalid',str(index),{key:Raw(param)},Raw('1e309'))
    equal_pairs = [('9007199254740992.0','9007199254740993'),('1e309','2e309'),('-0.0','0'),('true','1'),
        ('{"x":[1,false,-0.0]}','{"x":[1.0,0,0]}'),('{"x":[1,false,-0.0]}','{"x":[1.0,false,0]}'),
        ('{"a":1,"b":2}','{"b":2.0,"a":1.0}'),('{"a":1}','{"b":1}'),('[[1]]','[[1,2]]'),('[]','{}')]
    for index,(a,b) in enumerate(equal_pairs):
        prim('equal',str(index),Raw(a),Raw(b))
        for key,value in [('const',Raw(a)),('enum',[Raw(a)]),('enum-multi',[None,Raw(a)])]:
            add('equality',f'{index}/{key}',{key.split('-')[0]:value},Raw(b),list(DRAFTS)[1:] if key=='const' else None)
    unique = ['[1,1.0]','[true,1]','[false,0]','[1e309,2e309]','[1e309,-1e309]',
        '[9007199254740993,9007199254740992.0]','[{"x":[1]},{"x":[1.0]}]',
        '[null,"x",{},[],true,1]','[[true],[1]]']
    for index,token in enumerate(unique):
        add('unique',str(index),{'uniqueItems':True},Raw(token)); prim('unique',str(index),Raw(token))
    add('unique','disabled',{'uniqueItems':False},Raw('[1,1.0]'))
    add('unique','inapplicable',{'uniqueItems':True},{})
    for key in ('minLength','maxLength','minItems','maxItems','minProperties','maxProperties'):
        contents = ['', '😀','é','é'] if key.endswith('Length') else [[],[0],[0,1]] if key.endswith('Items') else [{},{'a':0},{'a':0,'b':1}]
        for p,param in enumerate(['0','1','1.0','1e0','1e-400','18446744073709551616','-1','1e309']):
            for c,content in enumerate(contents): add('length',f'{key}/{p}/{c}',{key:Raw(param)},content)
    for param in ['1.0','18446744073709551616','1e-400']:
        for n in range(3):
            add('contains-limit',param+'/'+str(n),{'contains':{},'minContains':Raw(param),'maxContains':Raw(param)},[{}]*n,list(DRAFTS)[3:])
    for index,token in enumerate(['[1,1.0]','[true,1]','[1e309,2e309]']):
        add('schema-enum',str(index),{'enum':Raw(token)},1)
    limit=sys.get_int_max_str_digits()
    assert limit == 4300, 'fixture policy must be reviewed if the installed limit changes'
    for n in (limit-1,limit,limit+1):
        prim('parse','digit/'+str(n),Raw('9'*n))
        add('parse-limit','root/'+str(n),{'type':'number'},Raw('9'*n))
    for label,content,schema in [('negative',Raw('-'+'9'*(limit+1)),{}),
        ('property',{'x':Raw('9'*(limit+1))},{}),('duplicate',Raw('{"x":'+'9'*(limit+1)+',"x":0}'),{}),
        ('default',{}, {'default':Raw('9'*(limit+1))})]: add('parse-limit',label,schema,content)
    for token in ('NaN','Infinity','-Infinity','01','1e','1.'):
        add('transport',token,{},Raw(token))
        if token in ('NaN','Infinity','-Infinity'):
            for row in rows[-10:]: row['transport_gap']='NonstandardConstant'
    A={'multipleOf':Raw('2.0')}; V={'multipleOf':Raw('1e309')}
    ordered={'maximum':0,**A}; reversed_order={**A,'maximum':0}
    trees=[('ordered',ordered),('reversed',reversed_order),('not',{'not':ordered}),('not-reversed',{'not':reversed_order}),
        ('any-hidden',{'anyOf':[{},A]}),('any-reached',{'anyOf':[A,{}]}),('any-exhaust',{'anyOf':[ordered,{}]}),
        ('one-A',{'oneOf':[{}, {},A]}),('one-V',{'oneOf':[{}, {},V]}),
        ('one-ref',{'definitions':{'a':A},'oneOf':[{}, {},{'$ref':'#/definitions/a'}]}),
        ('all',{'allOf':[{'maximum':0},A]}),('metadata',{'default':A,'unknown':A}),
        ('ref-sibling',{'definitions':{'ok':{}},'$ref':'#/definitions/ok',**A})]
    for label,schema in trees: add('order',label,schema,Raw('1e309'))
    for condition in (ordered,reversed_order):
        add('order','if/'+dump(condition),{'if':condition,'then':False,'else':True},Raw('1e309'),list(DRAFTS)[2:])
    for condition in ({'minimum':Raw('1e309'),'exclusiveMinimum':Raw('1e309'),**A},
                      {**A,'exclusiveMinimum':Raw('1e309'),'minimum':Raw('1e309')}):
        add('order','exclusive/'+dump(condition),condition,Raw('1e309'),list(DRAFTS)[1:])
    for present in (False,True): add('property','present/'+str(present),{'properties':{'x':A}},{'x':Raw('1e309')} if present else {})
    for token in ('1','1.0'):
        add('bound','exclusive-one/'+token,{'exclusiveMinimum':Raw('1.0')},Raw(token),list(DRAFTS)[1:])
    for arr in (["ok",Raw('1e309')],[Raw('1e309'),"ok"]):
        sub={'contains':{'anyOf':[{'type':'string'},A]}}
        add('contains-order',dump(arr),{'properties':{'x':sub}},{'x':arr},list(DRAFTS)[1:])
        add('contains-order','max-zero/'+dump(arr),{'properties':{'x':{**sub,'maxContains':Raw('0.0')}}},{'x':arr},list(DRAFTS)[3:])
    for parent,child in [('draft7','draft202012'),('draft202012','draft7')]:
        add('transition',child,{'definitions':{'ok':{}},'properties':{'x':{'$schema':DRAFTS[child],'$ref':'#/definitions/ok',**A}}},{'x':Raw('1e309')},[parent])
    assert len(rows)==len({r['name'] for r in rows})
    return rows


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for argument in ('binary','cases','full-cases','out'): parser.add_argument('--'+argument,required=True)
    parser.add_argument('--generate',action='store_true')
    parser.add_argument('--capture-only',action='store_true')
    args=parser.parse_args()
    versions,environment=assert_oracle_environment()
    verify_sources()
    manifest=json.loads(pathlib.Path('/work/crates/comandos-extensions/src/output_schema/traversal/numbers/PROVENANCE.json').read_text())
    for entry in manifest['sources']+manifest['notices']+manifest['cached_conversion_sources']+manifest['dependencies_archives']:
        source=pathlib.Path(entry['source'].replace('/home/someguy/.local/share/comandos/extensions-venv','/venv').replace('/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration','/work'))
        assert hashlib.sha256(source.read_bytes()).hexdigest()==entry['sha256'],source
    if args.generate:
        rows=directed_cases()
        for row in rows: row['oracle']=reference(row)
        pathlib.Path(args.cases).write_text(json.dumps(rows,indent=2,ensure_ascii=False)+'\n')
    rows=json.loads(pathlib.Path(args.cases).read_text())
    refs=[reference(row) for row in rows]
    for row,ref in zip(rows,refs): assert row['oracle']==ref,row['name']
    if args.capture_only:
        artifact={'versions':versions,'environment':environment,'cases':len(rows),'results':rows}
    else:
        sent=[]
        for row,ref in zip(rows,refs):
            skip=ref.get('instance',ref)['result'].startswith('not-run:') or row.get('transport_gap')
            if not skip:
                sent.append({**row,'oracle_checked':ref.get('schema',{}).get('result')=='valid'})
        actual,measurements=probe(args.binary,sent,'output_schema::traversal::tests::audit_probe','SCHEMA_TRAVERSAL=')
        # Schema-as-instance checking remains a separate, unchanged backend.
        # Keep its discrepancies even when the private assertion now agrees.
        schema_probes={}
        for row,ref in zip(rows,refs):
            if row['group'] in ('length','schema-enum','schema-invalid','contains-limit') and ref.get('parse',{}).get('result')=='valid':
                schema_probes.setdefault(row['schema_json'],(row,ref))
        schema_rows=[row for row,ref in schema_probes.values()]
        stages,stage_measurements=probe(args.binary,schema_rows,'output_schema::preflight::tests::audit_probe','SCHEMA_PREFLIGHT=')
        stage_results=[{'name':row['name'],'schema_json':row['schema_json'],'python':ref['schema']['result'],
            'native':stage['native'],'mismatch':(ref['schema']['result']=='valid')!=(stage['native']=='valid')}
            for (row,ref),stage in zip(schema_probes.values(),stages)]
        indexed={r['name']:r for r in actual}
        assert len(indexed)==len(sent) and set(indexed)=={r['name'] for r in sent},'probe omitted or duplicated rows'
        results=[]
        for row,ref in zip(rows,refs):
            expected=ref.get('instance',ref)['result']
            if row.get('transport_gap'): expected='not-run:transport-precondition'
            elif 'op' not in row and ref.get('schema',{}).get('result')!='valid' and not expected.startswith('not-run:parse'):
                expected='not-run:precondition'
            observed=indexed[row['name']] if row['name'] in indexed else {'result':expected,'phase':'prerequisite'}
            mismatch=observed.get('result',observed.get('native'))!=expected
            infrastructure=observed.get('result',observed.get('native','')).startswith('infrastructure:')
            selector_mismatch=(observed.get('root') != ('primitive' if 'op' in row else ref.get('root'))) if row['name'] in indexed else False
            unexpected_gap=observed.get('result','').startswith('ScopeGap:')
            unexpected_omission=observed.get('result','').startswith('not-run:') and not expected.startswith('not-run:')
            if 'op' in row and expected=='Primitive':
                mismatch |= {k:v for k,v in observed.items() if k not in ('name','root')} != ref
            results.append({**row,'native':observed,'expected':expected,'mismatch':mismatch,'selector_mismatch':selector_mismatch,
                'infrastructure_failure':infrastructure,'unexpected_gap':unexpected_gap,'unexpected_precondition_omission':unexpected_omission,
                'unresolved':expected.startswith('not-run:') or observed.get('result',observed.get('native','')).startswith(('ScopeGap:','infrastructure:'))})
        artifact={'versions':versions,'environment':environment,'resource_bounds':{'address_space_bytes':384*1024*1024,'cpu_seconds':[2,3],'batch_size':8},
            'full_cases_sha256':hashlib.sha256(pathlib.Path(args.full_cases).read_bytes()).hexdigest(),
            'cases_sha256':hashlib.sha256(pathlib.Path(args.cases).read_bytes()).hexdigest(),'measurements':measurements,
            'schema_stage_measurements':stage_measurements,'schema_stage_results':stage_results,
            'summary':{'cases':len(results),'mismatches':sum(r['mismatch'] for r in results),'unresolved':sum(r['unresolved'] for r in results),
                'schema_stage_mismatches':sum(r['mismatch'] for r in stage_results),
                'stage_infrastructure_failures':sum(r['native'].startswith('infrastructure:') for r in stage_results),
                **{key:sum(r[key] for r in results) for key in ('selector_mismatch','infrastructure_failure','unexpected_gap','unexpected_precondition_omission')}},'results':results}
    pathlib.Path(args.out).write_text(json.dumps(artifact,indent=2,ensure_ascii=False)+'\n')
    print(json.dumps(artifact.get('summary',{'captured':len(rows)})))
    return int(not args.capture_only and any(artifact['summary'][key] for key in ('mismatches','unresolved','selector_mismatch','infrastructure_failure','stage_infrastructure_failures','unexpected_gap','unexpected_precondition_omission')))


if __name__=='__main__':
    raise SystemExit(main())
