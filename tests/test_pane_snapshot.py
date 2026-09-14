import importlib.util
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def module():
    path = ROOT / 'lib' / 'pane_snapshot.py'
    assert path.exists(), 'missing per-pane snapshot inspector'
    spec = importlib.util.spec_from_file_location('pane_snapshot', path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def proc(root, pid, parent, argv, env=None):
    p = root / str(pid)
    p.mkdir(parents=True)
    (p / 'stat').write_text(f'{pid} (test) S {parent} 0 0')
    (p / 'cmdline').write_bytes(b'\0'.join(a.encode() for a in argv))
    (p / 'environ').write_bytes(b'\0'.join(f'{k}={v}'.encode() for k,v in (env or {}).items()))
    (p / 'fd').mkdir()
    return p


def rollout(directory, session_id, source="cli"):
    path = directory / f"rollout-2026-09-10-{session_id}.jsonl"
    path.write_text(json.dumps({"type": "session_meta", "payload": {
        "id": session_id, "session_id": session_id, "source": source,
    }}) + "\n")
    return path


def test_mixed_agents_in_same_session_do_not_inherit_each_others_identity(tmp_path):
    m = module()
    home = tmp_path / 'home'; home.mkdir()
    root = tmp_path / 'proc'; root.mkdir()
    proc(root, 1, 0, ['zsh']); proc(root, 2, 0, ['zsh'])
    cp = proc(root, 11, 1, ['codex', 'resume', '11111111-1111-1111-1111-111111111111'])
    (cp / 'fd' / '4').symlink_to(rollout(tmp_path, '11111111-1111-1111-1111-111111111111'))
    proc(root, 12, 2, ['claude', '--resume', 'other'])
    cfg = home / '.claude-accounts' / 'work'; (cfg / 'sessions').mkdir(parents=True)
    (cfg / 'sessions' / '12.json').write_text(json.dumps({'pid':12,'sessionId':'22222222-2222-2222-2222-222222222222','tmux':'test:@1.%2'}))
    inspect = m.PaneInspector(home=home, proc_root=root)
    a = inspect({'id':'%1','pid':1,'command':'node'})
    b = inspect({'id':'%2','pid':2,'command':'claude'})
    assert (a['agent'],a['resume_id']) == ('codex','11111111-1111-1111-1111-111111111111')
    assert (b['agent'],b['resume_id'],b['claude_config_dir']) == ('claude','22222222-2222-2222-2222-222222222222',str(cfg))


def test_grok_is_matched_by_live_pid_not_project_or_old_pane_number(tmp_path):
    m = module(); root = tmp_path/'proc'; root.mkdir(); home=tmp_path/'home';home.mkdir()
    proc(root, 1, 0, ['zsh']); proc(root, 2, 1, ['grok','--resume','old'])
    cfg=home/'.grok';cfg.mkdir()
    (cfg/'active_sessions.json').write_text(json.dumps([{'pid':2,'session_id':'live'},{'pid':44,'session_id':'stale'}]))
    assert m.PaneInspector(home=home,proc_root=root)({'id':'%4','pid':1,'command':'grok'})['resume_id']=='live'


def test_shell_with_background_agent_is_not_resumed_as_agent(tmp_path):
    m=module();root=tmp_path/'proc';root.mkdir();home=tmp_path/'home';home.mkdir()
    proc(root,1,0,['zsh']);proc(root,2,1,['codex','resume','old'])
    assert m.PaneInspector(home=home,proc_root=root)({'id':'%1','pid':1,'command':'zsh'})=={'agent':''}


def test_resume_flags_preserve_values_and_never_capture_prompt_text(tmp_path):
    m=module();root=tmp_path/'proc';root.mkdir();home=tmp_path/'home';home.mkdir()
    proc(root,1,0,['zsh'])
    p=proc(root,2,1,['codex','resume','11111111-1111-1111-1111-111111111111','-m','gpt-6-astra','--dangerously-bypass-approvals-and-sandbox','private prompt'])
    data=m.PaneInspector(home=home,proc_root=root)({'id':'%1','pid':1,'command':'node'})
    assert data['flags']==['--model','gpt-6-astra','--dangerously-bypass-approvals-and-sandbox']


def test_resume_flags_preserve_explicit_sandbox_approvals_and_config(tmp_path):
    m = module(); root = tmp_path / 'proc'; home = tmp_path / 'home'; home.mkdir()
    proc(root, 1, 0, ['codex', 'resume', 'exact', '--sandbox', 'read-only', '--ask-for-approval=untrusted',
                      '-c', 'approval_policy="never"', '-c', 'model_reasoning_effort="high"',
                      '--add-dir', '/tmp/allowed', 'private prompt'])
    flags = m.PaneInspector(home=home, proc_root=root)._flags(1)
    assert flags == ['--sandbox', 'read-only', '--ask-for-approval=untrusted', '-c', 'approval_policy="never"',
                     '-c', 'model_reasoning_effort="high"', '--add-dir', '/tmp/allowed']


def test_acp_snapshot_preserves_explicit_danger_mode(tmp_path):
    m = module(); root = tmp_path / 'proc'; home = tmp_path / 'home'; home.mkdir()
    proc(root, 1, 0, ['sh']); proc(root, 2, 1, ['cc-acp', '--agent', 'claude', '--danger'])
    hooks = home / '.claude' / 'hooks'; hooks.mkdir(parents=True)
    (hooks / 'acp-panes.json').write_text(json.dumps({'%1': {'pid': 2, 'sessionId': 'exact'}}))
    assert m.PaneInspector(home=home, proc_root=root)({'id': '%1', 'pid': 1, 'command': 'cc-acp'})['flags'] == ['--danger']


def test_codex_chooses_root_rollout_when_same_process_opens_subagent_rollout_first(tmp_path):
    m = module()
    root = tmp_path / "proc"
    home = tmp_path / "home"
    home.mkdir()
    process = proc(root, 10, 0, ["codex"])
    parent_id = "11111111-1111-1111-1111-111111111111"
    child_id = "22222222-2222-2222-2222-222222222222"
    child = rollout(tmp_path, child_id, {"subagent": {"thread_spawn": {"parent_thread_id": parent_id, "depth": 1}}})
    (process / "fd" / "37").symlink_to(child)
    (process / "fd" / "48").symlink_to(rollout(tmp_path, parent_id))
    inspected = m.PaneInspector(home=home, proc_root=root)({"id": "%1", "pid": 10, "command": "codex"})
    assert inspected["resume_id"] == parent_id


def test_codex_current_root_rollout_takes_precedence_over_original_resume_argument(tmp_path):
    m = module()
    root = tmp_path / "proc"
    home = tmp_path / "home"
    home.mkdir()
    old_id = "11111111-1111-1111-1111-111111111111"
    current_id = "22222222-2222-2222-2222-222222222222"
    process = proc(root, 10, 0, ["codex", "resume", old_id])
    (process / "fd" / "48").symlink_to(rollout(tmp_path, current_id))
    inspected = m.PaneInspector(home=home, proc_root=root)({"id": "%1", "pid": 10, "command": "codex"})
    assert inspected["resume_id"] == current_id


def test_codex_without_root_rollout_uses_explicit_resume_instead_of_subagent(tmp_path):
    m = module()
    root = tmp_path / "proc"
    home = tmp_path / "home"
    home.mkdir()
    parent_id = "11111111-1111-1111-1111-111111111111"
    child_id = "22222222-2222-2222-2222-222222222222"
    process = proc(root, 10, 0, ["codex", "resume", parent_id])
    child = rollout(tmp_path, child_id, {"subagent": {"thread_spawn": {"parent_thread_id": parent_id}}})
    (process / "fd" / "37").symlink_to(child)
    inspected = m.PaneInspector(home=home, proc_root=root)({"id": "%1", "pid": 10, "command": "codex"})
    assert inspected["resume_id"] == parent_id


def test_codex_without_identity_does_not_inherit_subprocess_conversation(tmp_path):
    m = module()
    root = tmp_path / "proc"
    home = tmp_path / "home"
    home.mkdir()
    proc(root, 10, 0, ["codex"])
    proc(root, 20, 10, ["codex", "resume", "22222222-2222-2222-2222-222222222222"])
    inspected = m.PaneInspector(home=home, proc_root=root)({"id": "%1", "pid": 10, "command": "codex"})
    assert inspected == {"agent": "codex"}


def test_claude_without_identity_does_not_inherit_subprocess_conversation(tmp_path):
    m = module()
    root = tmp_path / "proc"
    home = tmp_path / "home"
    home.mkdir()
    proc(root, 10, 0, ["claude"])
    proc(root, 20, 10, ["claude", "--resume", "child-conversation"])
    sessions = home / ".claude" / "sessions"
    sessions.mkdir(parents=True)
    (sessions / "20.json").write_text(json.dumps({"pid": 20, "sessionId": "child-conversation"}))
    inspected = m.PaneInspector(home=home, proc_root=root)({"id": "%1", "pid": 10, "command": "claude"})
    assert inspected == {"agent": "claude"}


def test_compiled_grok_alias_keeps_exact_conversation(tmp_path):
    m = module(); root = tmp_path/'proc'; root.mkdir(); home=tmp_path/'home';home.mkdir()
    proc(root, 1, 0, ['zsh']); proc(root, 2, 1, ['grok-linux-x86_64','--resume','old'])
    cfg=home/'.grok';cfg.mkdir()
    (cfg/'active_sessions.json').write_text(json.dumps([{'pid':2,'session_id':'live'}]))
    assert m.PaneInspector(home=home,proc_root=root)({'id':'%4','pid':1,'command':'grok-linux-x86_64'})['resume_id']=='live'


def test_codex_metadata_cache_reads_idle_rollout_once_and_invalidates_on_append(tmp_path):
    m = module()
    path = tmp_path / 'rollout.jsonl'
    path.write_text(json.dumps({'type': 'turn_context', 'payload': {'model': 'gpt-6-astra', 'effort': 'max'}}) + '\n')
    cache = m.CodexMetadataCache()
    reads = []
    original = cache._read_context
    def counted(*args):
        reads.append(args)
        return original(*args)
    cache._read_context = counted
    assert cache.read(12, 'root', path)['effort'] == 'max'
    assert cache.read(12, 'root', path)['effort'] == 'max'
    assert len(reads) == 1
    with path.open('a') as fh:
        fh.write(json.dumps({'type': 'turn_context', 'payload': {'model': 'gpt-6-astra', 'effort': 'ultra'}}) + '\n')
    assert cache.read(12, 'root', path)['effort'] == 'ultra'
    assert len(reads) == 2
    assert len(cache.entries) == 1


def test_codex_metadata_cache_handles_chunk_boundary_and_is_bounded(tmp_path):
    m = module()
    path = tmp_path / 'rollout.jsonl'
    row = json.dumps({'type': 'turn_context', 'payload': {'model': 'gpt-6-astra', 'effort': 'ultra'}})
    path.write_text(row + '\n' + json.dumps({'type': 'event_msg', 'text': 'x' * 200}) + '\n')
    cache = m.CodexMetadataCache(max_entries=2, chunk_bytes=64)
    for pid in (1, 2, 3):
        assert cache.read(pid, 'root', path)['effort'] == 'ultra'
    assert len(cache.entries) == 2
    assert all(key[0] in (2, 3) for key in cache.entries)


def test_codex_metadata_cache_invalidates_atomic_replacement(tmp_path):
    m = module()
    path = tmp_path / 'rollout.jsonl'
    path.write_text(json.dumps({'type': 'turn_context', 'payload': {'model': 'first'}}) + '\n')
    cache = m.CodexMetadataCache()
    assert cache.read(1, 'root', path)['model'] == 'first'
    replacement = tmp_path / 'new.jsonl'
    replacement.write_text(json.dumps({'type': 'turn_context', 'payload': {'model': 'other'}}) + '\n')
    replacement.replace(path)
    assert cache.read(1, 'root', path)['model'] == 'other'
