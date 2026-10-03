#!/usr/bin/env python3
"""Ordered traversal audit. Explicit gaps remain unresolved and exit nonzero."""
import argparse
import hashlib
import json
import pathlib
import resource
import subprocess
import time
import warnings
import jsonschema
from referencing import Registry
from referencing.exceptions import Unresolvable
from output_schema_audit import assert_oracle_environment, native, reference as sdk_reference

ROOT = pathlib.Path('/work')
CASES = pathlib.Path(__file__).with_name('output_schema_traversal_cases.json')
DRAFTS = {'draft4': 'http://json-schema.org/draft-04/schema#',
          'draft6': 'http://json-schema.org/draft-06/schema#',
          'draft7': 'http://json-schema.org/draft-07/schema#',
          'draft201909': 'https://json-schema.org/draft/2019-09/schema',
          'draft202012': 'https://json-schema.org/draft/2020-12/schema'}


def outcome(error):
    if isinstance(error, Unresolvable):
        category = 'Abort:Unresolvable'
    elif isinstance(error, jsonschema.exceptions.ValidationError):
        category = 'Invalid'
    else:
        category = 'Abort:' + type(error).__name__
    return {'result': category, 'exception': type(error).__name__,
            'unresolvable': isinstance(error, Unresolvable),
            'sdk_mapping': 'RuntimeError' if isinstance(error, (Unresolvable,
                jsonschema.exceptions.SchemaError, jsonschema.exceptions.ValidationError)) else type(error).__name__}


def oracle(row):
    schema, content = json.loads(row['schema_json']), json.loads(row['content_json'])
    with warnings.catch_warnings():
        warnings.simplefilter('ignore', DeprecationWarning)
        try:
            selected = jsonschema.validators.validator_for(schema)
        except Exception as error:
            return {'root': 'error:' + type(error).__name__, 'schema': outcome(error),
                    'instance': {'result': 'not-run:precondition'}}
        try:
            selected.check_schema(schema)
            stage = {'result': 'valid'}
        except Exception as error:
            return {'root': selected.__name__, 'schema': outcome(error),
                    'instance': {'result': 'not-run:precondition'}}
        try:
            if row.get('mode', 'ExhaustErrors') == 'FirstError':
                valid = selected(schema, registry=Registry()).is_valid(content)
            else:
                jsonschema.validate(content, schema, registry=Registry())
                valid = True
            result = {'result': 'Valid' if valid else 'Invalid',
                      'sdk_mapping': 'success' if valid else 'RuntimeError'}
        except Exception as error:
            result = outcome(error)
    return {'root': selected.__name__, 'schema': stage, 'instance': result}


def directed_cases():
    rows = []
    def add(name, schema, content=None, mode='ExhaustErrors', draft=None, gap=None):
        if draft:
            schema = {'$schema': DRAFTS[draft], **schema}
            name = draft + '/' + name
        row = {'name': name, 'schema_json': json.dumps(schema, separators=(',', ':'), ensure_ascii=False),
               'content_json': json.dumps({} if content is None else content, separators=(',', ':'), ensure_ascii=False),
               'mode': mode, 'expected_gap': gap}
        rows.append(row)
    missing = {'$ref': '#/definitions/missing'}
    required_first = {'required': ['x'], **missing}
    ref_first = {**missing, 'required': ['x']}
    minimal = [
        ('top-exhaust', required_first, {}, 'ExhaustErrors'),
        ('top-first', required_first, {}, 'FirstError'),
        ('not-required-first', {'not': required_first}, {}, 'ExhaustErrors'),
        ('not-ref-first', {'not': ref_first}, {}, 'ExhaustErrors'),
        ('if-required-first', {'if': required_first, 'then': False, 'else': True}, {}, 'ExhaustErrors'),
        ('if-ref-first', {'if': ref_first, 'then': False, 'else': True}, {}, 'ExhaustErrors'),
        ('any-failure-before-ref', {'anyOf': [required_first, {}]}, {}, 'FirstError'),
        ('any-success-before-ref', {'anyOf': [{}, missing]}, {}, 'ExhaustErrors'),
        ('one-two-successes-before-ref', {'oneOf': [{}, {}, missing]}, {}, 'ExhaustErrors'),
        ('absent-property', {'properties': {'x': missing}}, {}, 'ExhaustErrors'),
        ('present-property', {'properties': {'x': missing}}, {'x': {}}, 'ExhaustErrors'),
        ('productive-ref', {'type': 'object', 'properties': {'child': {'$ref': '#'}}}, {'child': {'child': {}}}, 'ExhaustErrors'),
        ('no-progress-ref', {'$ref': '#'}, {}, 'ExhaustErrors')]
    for draft in DRAFTS:
        for name, schema, content, mode in minimal:
            add(name, schema, content, mode, draft)
        add('sibling-filter', {'definitions': {'ok': {}}, '$ref': '#/definitions/ok', 'required': ['x']}, draft=draft)
        for mode in ('FirstError', 'ExhaustErrors'):
            add('allof-' + mode, {'allOf': [{'required': ['x']}, missing]}, mode=mode, draft=draft)
        add('invalid-ignored-sibling', {'$ref': '#', 'required': 1}, draft=draft)
        add('invalid-unused', {'properties': {'unused': {'type': 7}}}, draft=draft)
        add('anchor', {'definitions': {'ok': {('id' if draft == 'draft4' else '$id' if draft in ('draft6', 'draft7') else '$anchor'): '#ok' if draft in ('draft4', 'draft6', 'draft7') else 'ok'}}, '$ref': '#ok'}, draft=draft)
        add('missing-anchor', {'$ref': '#missing'}, draft=draft)
        add('unknown-keyword', {'extension': missing}, draft=draft)
        add('literal-metadata', {'default': {'$id': 'urn:literal', '$schema': 3, '$anchor': 'x', '$ref': '#/missing', 'number': 1.0}, 'examples': [missing]}, draft=draft)
        add('two-node-cycle', {'definitions': {'a': {'$ref': '#/definitions/b'}, 'b': {'$ref': '#/definitions/a'}}, '$ref': '#/definitions/a'}, draft=draft)
    for draft in ('draft6', 'draft7', 'draft201909', 'draft202012'):
        schema = {'contains': {'anyOf': [{'required': ['ok']}, missing]}}
        for reverse in (False, True):
            arr = [{'ok': True}, {}] if not reverse else [{}, {'ok': True}]
            add('contains-' + str(reverse), schema, arr, draft=draft)
            add('property-contains-' + str(reverse), {'properties': {'x': schema}}, {'x': arr}, draft=draft)
        if draft.startswith('draft20'):
            add('contains-max-zero', {**schema, 'maxContains': 0}, [{'ok': True}, {}], draft=draft)
            for label, limits, arr, gap in [
                ('empty-min-zero', {'minContains': 0}, [], None),
                ('empty-default', {}, [], None),
                ('min-two', {'minContains': 2}, [{}], None),
                ('max-one', {'maxContains': 1}, [{}, {}], None),
                ('float-bound', {'minContains': 1.0}, [{}], 'Numeric')]:
                add('contains-' + label, {'contains': {}, **limits}, arr, draft=draft, gap=gap)
    for reference in ('#/definitions/a%2Fb', '#/definitions/a~1b', '#/definitions/til~0de', '#/definitions/a+b', '#/definitions/percent%XX', '#/definitions/%FF'):
        add('pointer-' + reference, {'definitions': {'a/b': {}, 'til~de': {}, 'a+b': {}, 'percent%XX': {}}, '$ref': reference}, gap='PointerSemantics' if reference.endswith('%FF') else None)
    for index in ('-1', '+0', '00', '3', '١', '0_0', ' 0', '99999999999999999999999999999999999'):
        add('index-' + index, {'anyOf': [{}, {}], '$ref': '#/anyOf/' + index}, gap='PointerSemantics' if index in ('١', '0_0', ' 0', '99999999999999999999999999999999999') else None)
    for name, schema, gap in [
        ('duplicate-anchor', {'definitions': {'a': {'$anchor': 'same'}, 'b': {'$anchor': 'same'}}, '$ref': '#same'}, 'DuplicateAnchor'),
        ('dynamic-anchor', {'definitions': {'a': {'$dynamicAnchor': 'same'}}, '$ref': '#same'}, 'DynamicScope'),
        ('missing-anchor-id', {'definitions': {'a': {'$id': 'urn:other'}}, '$ref': '#missing'}, 'ResourceId'),
        ('external-absent', {'$ref': 'urn:missing'}, None),
        ('external-absent-property', {'properties': {'x': {'$ref': 'urn:missing'}}}, None),
        ('external-id', {'definitions': {'a': {'$id': 'urn:other'}}, '$ref': 'urn:missing'}, 'ResourceId'),
        ('bundled-http', {'$ref': 'http://json-schema.org/draft-04/schema'}, 'ExternalResource'),
        ('unregistered-https', {'$ref': 'https://json-schema.org/draft-04/schema'}, None),
        ('external-fragment', {'$ref': 'urn:missing#x'}, 'UriJoin'),
        ('const-literal', {'const': {'$schema': 'literal', '$ref': '#/missing', 'n': 1.0}}, 'Keyword'),
        ('enum-literal', {'enum': [{'$id': 'urn:literal'}]}, 'Keyword'),
        ('pattern', {'pattern': 'x'}, 'PythonRegex'),
        ('items', {'items': {}}, 'Keyword'),
        ('unevaluated', {'unevaluatedProperties': False}, 'Annotation'),
        ('number-type', {'type': 'number'}, 'Numeric'),
        ('unknown-root-cycle', {'$schema': 'unknown', '$ref': '#'}, None),
        ('crawl-unknown-dialect', {'definitions': {'a': {'$schema': 'unknown'}}, '$ref': '#missing'}, 'ResourceDialect'),
        ('crawl-noncanonical-dialect', {'definitions': {'a': {'$schema': 'HTTP://json-schema.org/draft-07/schema#'}}, '$ref': '#missing'}, 'ResourceDialect'),
        ('literal-pointer-id', {'default': {'$id': 'urn:literal', 'required': ['x']}, '$ref': '#/default'}, None),
        ('root-id', {'$id': 'urn:root'}, 'ResourceId'),
        ('draft3', {'$schema': 'http://json-schema.org/draft-03/schema#'}, 'Draft3')]:
        add(name, schema, '' if name == 'pattern' else [] if name == 'items' else {}, gap=gap)
    add('draft7-unknown-child', {'properties': {'x': {'$schema': 'unknown', 'prefixItems': [False]}}}, {'x': []}, draft='draft7')
    add('draft7-modern-child', {'properties': {'x': {'$schema': DRAFTS['draft202012'], 'prefixItems': [False]}}}, {'x': []}, draft='draft7', gap='Keyword')
    for parent, child in (('draft7', 'draft202012'), ('draft202012', 'draft7')):
        # Descend uses parent's filter, evolve uses child's own filter.
        sub = {'$schema': DRAFTS[child], 'required': ['x'], '$ref': '#/definitions/ok'}
        add('cross-properties', {'definitions': {'ok': {}}, 'properties': {'p': sub}}, {'p': {}}, draft=parent)
        add('cross-not', {'definitions': {'ok': {}}, 'not': sub}, {}, draft=parent)
        subnested = {**sub, 'properties': {'p': {'required': ['y'], '$ref': '#/definitions/ok'}}}
        add('cross-nested', {'definitions': {'ok': {}}, 'properties': {'p': subnested}}, {'p': {'p': {}}}, draft=parent)
    for depth in (1, 8, 24, 48, 63, 64, 96, 160):
        content = {}
        for _ in range(depth):
            content = {'child': content}
        add('productive-depth-' + str(depth), {'properties': {'child': {'$ref': '#'}}}, content, gap='BudgetBoundary' if depth >= 64 else None)
        if depth >= 64: rows[-1]['gap_phase'] = 'input' if depth == 160 else 'evaluation'
    add('empty-id-registry-snapshot', {'$defs': {'a': {'$id': '', '$anchor': 'a', 'properties': {'x': {'$ref': '#'}}}}, '$ref': '#a', 'required': ['root']}, {'root': True, 'x': {}})
    add('modern-to-draft4-empty-required', {'properties': {'x': {'$schema': DRAFTS['draft4'], 'required': []}}}, {'x': {}}, gap='FragmentShape')
    add('same-container-empty-id-order', {'$defs': {'a': {'$id': '', 'required': ['a']}, 'b': {'$id': '', 'required': ['b']}, 'go': {'$anchor': 'go', 'properties': {'x': {'$ref': '#'}}}}, '$ref': '#go', 'required': ['root']}, {'root': True, 'x': {'a': True}})
    add('different-container-empty-id-gap', {'$defs': {'a': {'$id': ''}, 'go': {'$anchor': 'go'}}, 'properties': {'b': {'$id': ''}}, '$ref': '#go'}, {}, gap='ResourceId')
    for draft in DRAFTS:
        key = 'id' if draft == 'draft4' else '$id' if draft in ('draft6', 'draft7') else '$anchor'
        anchor = {key: '#target' if draft in ('draft4', 'draft6', 'draft7') else 'target'}
        groups = [('crawl-items-schema', {'items': anchor, '$ref': '#target'}, None)]
        if draft != 'draft202012': groups.append(('crawl-items-tuple', {'items': [anchor], '$ref': '#target'}, None))
        if draft in ('draft4', 'draft6', 'draft7'):
            groups.extend([
                ('crawl-dependencies-first-schema', {'dependencies': {'one': anchor, 'two': {}}, '$ref': '#target'}, None),
                ('crawl-dependencies-first-array', {'dependencies': {'one': ['x'], 'two': anchor}, '$ref': '#target'}, None),
                ('crawl-dependencies-mixed-array', {'dependencies': {'one': anchor, 'two': ['x']}, '$ref': '#target'}, 'ResourceId')])
        for name, schema, gap in groups: add(name, schema, draft=draft, gap=gap)
    add('anchor-percent-not-decoded', {'definitions': {'a': {'$anchor': 'ok'}}, '$ref': '#%6Fk'})
    add('missing-anchor-slash', {'$ref': '#missing/name'})
    add('pointer-scalar', {'definitions': {'a': True}, '$ref': '#/definitions/a/x'}, gap='PointerSemantics')
    for name, schema in [('root-null', None), ('root-number', 1), ('root-list', []), ('root-string', 'schema'), ('root-list-with-key', ['$schema']), ('root-string-with-key', 'text $schema text'), ('root-true', True), ('root-false', False)]:
        add(name, schema)
    assert len({row['name'] for row in rows}) == len(rows)
    return rows


def verify_sources():
    provenance = json.loads((ROOT / 'crates/comandos-extensions/src/output_schema/traversal/PROVENANCE.json').read_text())
    for row in provenance['sources'] + provenance['notices']:
        source = pathlib.Path(row['source'].replace('/home/someguy/.local/share/comandos/extensions-venv', '/venv'))
        assert hashlib.sha256(source.read_bytes()).hexdigest() == row['sha256'], source
        if 'bundled' in row:
            bundled = pathlib.Path(row['bundled'].replace('/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration', '/work'))
            assert bundled.read_bytes() == source.read_bytes()
    from jsonschema_specifications import REGISTRY
    bundled = json.loads((ROOT / 'crates/comandos-extensions/src/output_schema/metaschemas/PROVENANCE.json').read_text())
    assert set(REGISTRY) == {row['uri'].rstrip('#') for row in bundled['files']}
    for cls in (jsonschema.Draft4Validator, jsonschema.Draft6Validator, jsonschema.Draft7Validator, jsonschema.Draft201909Validator, jsonschema.Draft202012Validator):
        assert sorted(cls.VALIDATORS) == provenance['keyword_inventories'][cls.__name__]


def probe(binary, rows, test, prefix):
    measurements = []
    actual = []
    for start in range(0, len(rows), 8):
        batch = rows[start:start + 8]
        before = resource.getrusage(resource.RUSAGE_CHILDREN)
        began = time.monotonic()
        try:
            result = subprocess.run([binary, '--exact', test, '--ignored', '--nocapture'],
                input=json.dumps(batch).encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
            assert result.returncode == 0 and not result.stderr, (result.returncode, result.stderr.decode(), result.stdout.decode())
            lines = [line[len(prefix):] for line in result.stdout.decode().splitlines() if line.startswith(prefix)]
            if prefix == 'SCHEMA_PREFLIGHT=':
                if len(lines) != 1:
                    raise ValueError('expected one schema-stage array')
                values = json.loads(lines[0])
                field = 'native'
            elif prefix == 'SCHEMA_TRAVERSAL=':
                values = [json.loads(line) for line in lines]
                field = 'result'
            else:
                raise ValueError('unknown probe protocol')
            if not isinstance(values, list) or len(values) != len(batch):
                raise ValueError('missing or extra probe rows')
            for row, value in zip(batch, values):
                if (not isinstance(value, dict) or value.get('name') != row['name']
                        or not isinstance(value.get(field), str) or not value[field]
                        or ('root' in value and not isinstance(value['root'], str))):
                    raise ValueError('malformed or mismatched probe row')
                if (field == 'result' and 'root' not in value
                        and not (value['result'] == 'ScopeGap:BudgetBoundary' and value.get('phase') == 'input')):
                    raise ValueError('missing traversal selector measurement')
            actual.extend(values)
            infrastructure = None
        except (subprocess.TimeoutExpired, AssertionError, ValueError, OSError) as error:
            infrastructure = type(error).__name__
            actual.extend({'name': row['name'], 'native': 'infrastructure:' + infrastructure} for row in batch)
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        measurements.append({'start': start, 'count': len(batch), 'seconds': time.monotonic() - began,
            'cpu_seconds': after.ru_utime + after.ru_stime - before.ru_utime - before.ru_stime,
            'child_peak_rss_kib_cumulative': after.ru_maxrss, 'infrastructure': infrastructure})
    return actual, measurements


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--stage-binary', required=True)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--cases', required=True)
    parser.add_argument('--full-cases', required=True)
    parser.add_argument('--out', required=True)
    parser.add_argument('--capture-only', action='store_true')
    parser.add_argument('--generate', action='store_true')
    parser.add_argument('--full', action='store_true')
    args = parser.parse_args()
    versions, environment = assert_oracle_environment()
    if args.generate:
        cases = directed_cases()
        for row in cases:
            row['oracle'] = oracle(row)
        pathlib.Path(args.cases).write_text(json.dumps(cases, indent=2, ensure_ascii=False) + '\n')
    cases = json.loads(pathlib.Path(args.full_cases if args.full else args.cases).read_text())
    if args.capture_only:
        results = [{'name': row['name'], **oracle(row)} for row in cases]
        pathlib.Path(args.out).write_text(json.dumps({'versions': versions, 'environment': environment, 'results': results}, indent=2) + '\n')
        return 0
    verify_sources()
    references = [oracle(row) for row in cases]
    for row, ref in zip(cases, references):
        if 'oracle' in row: assert row['oracle'] == ref, row['name']
    sent = [{**row, 'oracle_checked': ref['schema']['result'] == 'valid'} for row, ref in zip(cases, references)]
    stages, stage_measurements = probe(args.stage_binary, cases, 'output_schema::preflight::tests::audit_probe', 'SCHEMA_PREFLIGHT=')
    traversal, measurements = probe(args.stage_binary, sent, 'output_schema::traversal::tests::audit_probe', 'SCHEMA_TRAVERSAL=')
    rows = []
    for case, ref, stage, result in zip(cases, references, stages, traversal):
        assert stage['name'] == result['name'] == case['name']
        stock = native(args.binary, case['schema_json'], case['content_json'])
        sdk = sdk_reference(case['schema_json'], case['content_json'])
        evaluation = result.get('result', result.get('native'))
        if stage['native'] == 'rejected':
            composition = 'invalid-schema'
        elif stage['native'] != 'valid':
            composition = stage['native']
        elif ref['schema']['result'] != 'valid':
            composition = 'unresolved:precondition'
        else:
            composition = {'Valid': 'valid', 'Invalid': 'invalid-content'}.get(evaluation, evaluation)
        expected = ref['instance']['result'] if ref['schema']['result'] == 'valid' else 'invalid-schema'
        exact = {'Valid': 'valid', 'Invalid': 'invalid-content'}.get(expected, expected)
        gap = evaluation.startswith('ScopeGap:')
        unresolved = gap or evaluation.startswith(('not-run:', 'infrastructure:'))
        category_mismatch = not unresolved and evaluation != expected
        if ref['schema']['result'] != 'valid':
            category_mismatch = False
        expected_gap = case.get('expected_gap')
        unexpected_gap = gap and evaluation != 'ScopeGap:' + expected_gap if expected_gap else gap and not args.full
        gap_missing = bool(expected_gap) and ref['schema']['result'] == 'valid' and evaluation != 'ScopeGap:' + expected_gap
        row = {'name': case['name'], 'schema_json': case['schema_json'], 'content_json': case['content_json'],
            'mode': case.get('mode', 'ExhaustErrors'), 'python': ref, 'native_root': result.get('root'),
            'native_preflight': stage['native'], 'native_traversal': evaluation, 'native_pointer': result.get('pointer'),
            'native_sdk_mapping': 'success' if evaluation == 'Valid' else 'RuntimeError' if evaluation in ('Invalid', 'Abort:Unresolvable') else evaluation.removeprefix('Abort:') if evaluation.startswith('Abort:') else 'unresolved',
            'stock': stock, 'python_sdk': sdk, 'composition': composition, 'scope_gap': gap, 'traversal_phase': result.get('phase'),
            'category_mismatch': category_mismatch, 'unexpected_gap': unexpected_gap, 'missing_expected_gap': gap_missing,
            'preflight_mismatch': (stage['native'] == 'valid') != (ref['schema']['result'] == 'valid'),
            'selector_mismatch': result.get('root') not in (ref['root'], 'ScopeGap:Draft3', 'ScopeGap:DialectFailure') if 'root' in result else False,
            'unexpected_precondition_omission': ref['schema']['result'] == 'valid' and evaluation.startswith('not-run:'),
            'infrastructure_failure': evaluation.startswith('infrastructure:'),
            'stage_infrastructure_failure': stage['native'].startswith('infrastructure:'),
            'lost_coverage': sdk == stock and (gap or composition.startswith('unresolved:')),
            'new_false_reject_vs_stock': sdk == stock == 'valid' and composition in ('invalid-schema', 'invalid-content'),
            'new_false_accept_vs_stock': sdk != 'valid' and stock != 'valid' and composition == 'valid',
            'false_accept': composition == 'valid' and exact != 'valid',
            'false_reject': exact == 'valid' and composition in ('invalid-schema', 'invalid-content'),
            'stock_acceptance_mismatch': (stock == 'valid') != (sdk == 'valid'),
            'stock_classification_difference': stock != sdk}
        rows.append(row)
    summary = {key: sum(bool(row[key]) for row in rows) for key in (
        'scope_gap', 'category_mismatch', 'unexpected_gap', 'missing_expected_gap', 'preflight_mismatch',
        'selector_mismatch', 'lost_coverage', 'false_accept', 'false_reject', 'stock_acceptance_mismatch', 'stock_classification_difference', 'new_false_reject_vs_stock', 'new_false_accept_vs_stock')}
    summary.update({'cases': len(rows), 'supported_cohort_pass': not any(
        r['category_mismatch'] or r['unexpected_gap'] or r['missing_expected_gap'] or r['selector_mismatch']
        or r['unexpected_precondition_omission'] or r['infrastructure_failure'] or r['stage_infrastructure_failure']
        for r in rows),
        'precondition_omissions': sum(r['native_traversal'] == 'not-run:precondition' for r in rows),
        'unexpected_precondition_omissions': sum(r['unexpected_precondition_omission'] for r in rows),
        'infrastructure_failures': sum(r['infrastructure_failure'] for r in rows),
        'stage_infrastructure_failures': sum(r['stage_infrastructure_failure'] for r in rows),
        'semantic_results': {tag: sum(r['native_traversal'] == tag for r in rows) for tag in ('Valid', 'Invalid', 'Abort:Unresolvable', 'Abort:RecursionError')},
        'protective_limits': sum(r['native_traversal'] == 'ScopeGap:BudgetBoundary' for r in rows)})
    artifact = {'versions': versions, 'environment': environment, 'summary': summary,
        'resource_bounds': {'address_space_bytes': 384 * 1024 * 1024, 'cpu_seconds': [2, 3], 'batch_size': 8},
        'traversal_measurements': measurements, 'stage_measurements': stage_measurements, 'results': rows}
    pathlib.Path(args.out).write_text(json.dumps(artifact, indent=2) + '\n')
    print(json.dumps(summary))
    return int(bool(summary['scope_gap'] or summary['category_mismatch'] or summary['false_accept']
        or summary['false_reject'] or summary['infrastructure_failures'] or summary['stage_infrastructure_failures']
        or summary['preflight_mismatch'] or summary['selector_mismatch'] or summary['unexpected_gap']
        or summary['missing_expected_gap'] or summary['unexpected_precondition_omissions']))


if __name__ == '__main__':
    raise SystemExit(main())
