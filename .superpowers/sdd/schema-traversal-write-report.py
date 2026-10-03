import json,pathlib,re
root=pathlib.Path('/work');sdd=root/'.superpowers/sdd';direct=json.loads((sdd/'schema-traversal-directed.json').read_text());inv=json.loads((sdd/'schema-traversal-invariance.json').read_text());summary=direct['summary']
text=(sdd/'schema-traversal-report-template.txt').read_text()
values={'DIRECTED_ROWS':summary['cases'],'DIRECTED_SEMANTIC':sum(summary['semantic_results'].values()),'DIRECTED_GAPS':summary['scope_gap'],'DIRECTED_OMISSIONS':summary['precondition_omissions'],'DIRECTED_FALSE_REJECT':summary['false_reject'],'DIRECTED_LOST':summary['lost_coverage']}
for prefix,cohort,kind in [('D_STAGE','directed_metrics','stage_measurements'),('D_TRAV','directed_metrics','traversal_measurements'),('F_STAGE','full_metrics','stage_measurements'),('F_TRAV','full_metrics','traversal_measurements')]:
    row=inv[cohort][kind]
    for suffix,key in [('BATCHES','batches'),('WALL','max_wall_seconds'),('CPU','max_cpu_seconds'),('SUM','sum_cpu_seconds'),('RSS','cumulative_child_max_rss_kib')]:
        value=row[key];values[prefix+'_'+suffix]=f'{value:.4f}' if isinstance(value,float) else value
for key,value in values.items():text=text.replace('@@'+key+'@@',str(value))
text=text.replace('1 false rejects', '1 false reject')
for depth in [48,63,64,96,160,260]: text=text.replace('depth'+str(depth), 'depth '+str(depth))
assert '@@' not in text
needle='One cached Cargo no-run artifact-discovery command overlapped ordinary pytest'
addition='''The pre-selector captures are retained at /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-directed-pre-selector.json and /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-full-pre-selector.json. Their frozen hashes remain at /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration/.superpowers/sdd/schema-traversal-frozen-pre-selector.json. The final focused, formatting, clippy, full Rust, release and dependent differential checks ran after the selector correction. Ordinary pytest is the earlier passing run; the selector change adds only private Rust behavior and explicit audit fixtures.

'''
text=text.replace(needle,addition+needle)
assert '—' not in text and '–' not in text
paths=re.findall(r'/home/someguy/[^\s;|`]+',text)
for original in paths:
    original=original.rstrip('.)')
    local=original.replace('/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-migration','/work').replace('/home/someguy/.local/share/comandos/extensions-venv','/venv')
    if original=='/home/someguy/.cache/comandos/tiktoken':local='/tokens'
    assert pathlib.Path(local).exists(),original
(sdd/'schema-traversal-report.md').write_text(text)
print('Report written:',len(text.splitlines()),'lines;',len(paths),'absolute path references checked')
