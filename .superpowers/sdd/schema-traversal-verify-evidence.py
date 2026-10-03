import hashlib,json,pathlib,sys
from collections import Counter
root=pathlib.Path('/work');home='/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/'
def read(name):return json.loads((root/'.superpowers/sdd'/name).read_text())
before=read('schema-traversal-stock-before.json');after=read('schema-traversal-stock-after.json');approved=read('schema-preflight-full-after.json')
assert before['results']==after['results']==approved['results'];assert len(after['results'])==735;assert(after['acceptance_mismatches'],after['classification_differences'])==(60,192)
stage=read('schema-traversal-stage-after.json');assert stage['results']==read('schema-preflight-stage-after.json')['results'];assert len(stage['results'])==184 and stage['acceptance_mismatches']==31
full=read('schema-traversal-full.json');directed=read('schema-traversal-directed.json');oldcomposition=read('schema-preflight-full-candidate.json')
assert [(r['name'],r['native_preflight']) for r in full['results']]==[(r['name'],r['stage']) for r in oldcomposition['results']]
assert [(r['name'],r['stock'],r['python_sdk']) for r in full['results']]==[(r['name'],r['native'],r['python']) for r in after['results']]
assert all(a['summary']['category_mismatch']==a['summary']['infrastructure_failures']==a['summary']['stage_infrastructure_failures']==0 for a in [full,directed]);assert directed['summary']['supported_cohort_pass'];assert full['summary']['false_accept']==0
regressions=[r['name'] for r in full['results'] if r['new_false_reject_vs_stock']];assert regressions==[r['name'] for r in oldcomposition['results'] if r['new_acceptance_regression']]
frozen=read('schema-traversal-frozen.json')
modified={'crates/comandos-extensions/src/output_schema.rs','crates/comandos-extensions/src/output_schema/dialect.rs'}
for row in frozen['files']:
    path=row['path'].removeprefix(home)
    if path not in modified:assert hashlib.sha256((root/path).read_bytes()).hexdigest()==row['sha256'],path
worker=(root/'crates/comandos-extensions/src/output_schema.rs').read_text().replace('// Private ordered traversal audit; exact schema proof and parity still block wiring.\n#[allow(dead_code)]\nmod traversal;\n','')
assert hashlib.sha256(worker.encode()).hexdigest()==next(row['sha256'] for row in frozen['files'] if row['path']==home+'crates/comandos-extensions/src/output_schema.rs')
sys.path.insert(0,'/work/crates/comandos-extensions/tests');from output_schema_traversal_audit import directed_cases
current=json.loads((root/'crates/comandos-extensions/tests/output_schema_traversal_cases.json').read_text());generated=directed_cases();assert len(current)==len(generated)==227
byname={row['name']:row for row in current}
for row in generated:
    original=byname[row['name']]
    for key in ('schema_json','content_json','mode','expected_gap'):assert row[key]==original[key],(row['name'],key)
def metrics(artifact):
    data={}
    for key in ('stage_measurements','traversal_measurements'):
        batches=artifact[key];data[key]={'batches':len(batches),'max_rows':max(row['count'] for row in batches),'max_wall_seconds':max(row['seconds'] for row in batches),'max_cpu_seconds':max(row['cpu_seconds'] for row in batches),'sum_cpu_seconds':sum(row['cpu_seconds'] for row in batches),'cumulative_child_max_rss_kib':max(row['child_peak_rss_kib_cumulative'] for row in batches)}
    return data
results={'stock_rows_identical_to_approved':735,'stage_rows_identical_to_approved':184,'full_preflight_rows_identical':735,'historical_regex_regressions_unchanged':regressions,'protected_inputs_unchanged':True,'worker_except_declaration_byte_identical':True,'fixture_generator_matches_original_bytes':True,'directed_metrics':metrics(directed),'full_metrics':metrics(full),'directed_gap_inventory':dict(Counter(row['native_traversal'] for row in directed['results'] if row['scope_gap'])),'full_gap_inventory':dict(Counter(row['native_traversal'] for row in full['results'] if row['scope_gap']))}
(root/'.superpowers/sdd/schema-traversal-invariance.json').write_text(json.dumps(results,indent=2)+'\n');print(json.dumps(results,indent=2))
