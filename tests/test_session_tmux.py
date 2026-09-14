"""Real tmux/process/metadata boundary with a compiled, non-networking fake Codex.

No provider is launched. HOME, socket, account files and project are temporary.
"""
import copy
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import time
from types import SimpleNamespace
import uuid

import pytest

from test_session_route_matrix import REGISTRY
from test_agent_launch import load_dash_module
from session_operations import OperationStore, run_operation

SID = '11111111-1111-1111-1111-111111111111'


@pytest.fixture
def live(tmp_path, monkeypatch):
    if not shutil.which('tmux') or not shutil.which('cc'):
        pytest.skip('tmux and a C compiler required for private process integration')
    monkeypatch.setenv('HOME', str(tmp_path))
    monkeypatch.setenv('SHELL', '/bin/sh')
    monkeypatch.delenv('TMUX', raising=False)
    source = tmp_path / 'stub.c'
    binary = tmp_path / 'codex'
    source.write_text(r'''
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {
  const char *sid="11111111-1111-1111-1111-111111111111", *model="gpt-5.5";
  char effort[40]="high", path[4096];
  for (int i=1; i+1<argc; i++) {
    if (!strcmp(argv[i],"resume")) sid=argv[++i];
    else if (!strcmp(argv[i],"-m")) model=argv[++i];
    else if (!strcmp(argv[i],"-c")) sscanf(argv[++i],"model_reasoning_effort=\"%39[^\"]",effort);
  }
  if (!strcmp(model,"gpt-5.6-luna")) return 23;
  snprintf(path,sizeof(path),"%s/sessions/rollout-test-%s.jsonl",getenv("CODEX_HOME"),sid);
  FILE *f=fopen(path,"a+"); if (!f) return 24;
  fseek(f,0,SEEK_END);
  if (!ftell(f)) fprintf(f,"{\"type\":\"session_meta\",\"payload\":{\"id\":\"%s\",\"source\":\"cli\"}}\n",sid);
  fprintf(f,"{\"type\":\"turn_context\",\"payload\":{\"model\":\"%s\",\"effort\":\"%s\"}}\n",model,effort);
  fflush(f);
  const char *operation=getenv("COMANDOS_OPERATION_ID");
  if (operation && !strcmp(operation,"slow-redraw-operation")) sleep(3);
  printf("\033[2J\033[H> %s %s\n",model,effort); fflush(stdout);
  for (;;) pause();
}
''')
    built = subprocess.run(['cc', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=15)
    assert built.returncode == 0, built.stderr
    registry = copy.deepcopy(REGISTRY)
    for name, spec in registry['harnesses'].items():
        if spec['capabilities'].get('accounts'):
            spec.update(defaultHome=str(tmp_path / ('.' + name)), accountsRoot=str(tmp_path / ('.' + name + '-accounts')))
    for directory in (tmp_path / '.codex', tmp_path / '.codex-accounts/work'):
        (directory / 'sessions').mkdir(parents=True)
        (directory / 'auth.json').write_text(json.dumps({'tokens': {'access_token': 'fixture'}}))
    socket = 'comandos-operations-' + uuid.uuid4().hex
    env = dict(os.environ, PS1='$ ')
    def tmux(*args):
        return subprocess.run(['tmux', '-L', socket, '-f', '/dev/null', *map(str, args)],
                              capture_output=True, text=True, timeout=10, env=env)
    def checked(*args):
        result = tmux(*args)
        assert result.returncode == 0, result.stderr
        return result.stdout.strip()
    pane = checked('new-session', '-d', '-P', '-F', '#{pane_id}', '-s', 'audit', '-c', str(tmp_path), '/bin/sh')
    try:
        other = checked('split-window', '-h', '-d', '-P', '-F', '#{pane_id}', '-t', pane, '-c', str(tmp_path), '/bin/sh')
        dash = load_dash_module()
        monkeypatch.setattr(dash, 'tmux', tmux)
        monkeypatch.setattr(dash, 'read_conf', lambda: {})
        monkeypatch.setattr(dash, 'load_provider_registry', lambda: registry)
        monkeypatch.setattr(dash, '_harness_bin', lambda harness: str(binary) if harness == 'codex' else '/missing/' + harness)
        facts = {'harnesses': {'codex': {'available': True, 'authenticated': True}}, 'motors': {}, 'gateway': {}}
        monkeypatch.setattr(dash, 'capability_matrix', lambda: dash.provider_registry.evaluate_capability_matrix(registry, facts))
        monkeypatch.setattr(dash, 'time', SimpleNamespace(time=time.time, sleep=lambda seconds: time.sleep(min(seconds, .04))))
        store = OperationStore(tmp_path / 'operations.sqlite3')
        monkeypatch.setattr(dash, 'session_operation_store', lambda: store)
        command = dash._configuration_command('codex', 'codex', 'gpt-5.5', 'high', 'main', SID,
                                              ['--sandbox', 'read-only', '--ask-for-approval', 'untrusted'])
        dash._send_shell_line(pane, command)
        deadline = time.monotonic() + 5
        observed = {}
        while time.monotonic() < deadline:
            observed = dash.observe_pane('audit', pane)
            if observed.get('conversationId') == SID and observed.get('confirmed'):
                break
            time.sleep(.05)
        assert observed.get('confirmed'), observed
        yield SimpleNamespace(dash=dash, pane=pane, other=other, checked=checked, store=store, tmp_path=tmp_path)
    finally:
        tmux('kill-server')


@pytest.mark.parametrize('mode', ['change', 'account', 'launch-failure'])
def test_real_tmux_adapter_switch_and_recovery_preserve_other_pane(live, mode):
    dash = live.dash
    before = live.checked('display-message', '-p', '-t', live.other, '#{pane_pid}\t#{pane_current_command}')
    layout = live.checked('display-message', '-p', '-t', live.pane, '#{window_layout}')
    data = dict(session='audit', pane=live.pane, requestId='private-tmux-operation', toHarness='codex',
                motor='codex', model='gpt-5.6-luna' if mode == 'launch-failure' else 'gpt-5.5', effort='low',
                harnessAccount='work' if mode == 'account' else 'main',
                motorAccount='work' if mode == 'account' else 'main', interrupt=True)
    identity = dash._pane_identity('audit', live.pane)
    adapter = dash.SessionConfiguration(data, identity)
    adapter.store = live.store
    live.store.claim(data['requestId'], dash._identity_key(identity), data)
    result = run_operation(live.store, data['requestId'], adapter)
    assert result['ok'] is (mode != 'launch-failure'), result
    assert result['observed']['conversationId'] == SID
    assert result['observed']['confirmed'] is True
    assert result['observed']['effort'] == ('high' if mode == 'launch-failure' else 'low')
    if mode == 'launch-failure':
        assert result['rolledBack'] is True
    if mode == 'account':
        assert result['observed']['harnessAccount'] == result['observed']['motorAccount'] == 'work'
        assert (live.tmp_path / '.codex-accounts/work/sessions' / f'rollout-test-{SID}.jsonl').exists()
    args = dash._proc_cmdline(dash.agent_info_for_pane(live.pane)['pid'])
    assert args[args.index('--sandbox') + 1] == 'read-only'
    assert args[args.index('--ask-for-approval') + 1] == 'untrusted'
    assert live.checked('display-message', '-p', '-t', live.other, '#{pane_pid}\t#{pane_current_command}') == before
    assert live.checked('display-message', '-p', '-t', live.pane, '#{window_layout}') == layout


def test_old_prompt_cannot_confirm_delayed_new_cli(live):
    dash = live.dash
    live.checked('resize-window', '-t', live.pane, '-x', '300', '-y', '50')
    data = dict(session='audit', pane=live.pane, requestId='slow-redraw-operation', toHarness='codex',
                motor='codex', model='gpt-5.5', effort='high', harnessAccount='work', motorAccount='work', interrupt=True)
    identity = dash._pane_identity('audit', live.pane)
    adapter = dash.SessionConfiguration(data, identity); adapter.store = live.store
    adapter.verify = lambda plan, snapshot: adapter._verify(plan, plan.get('expectedSid', ''), attempts=2)
    live.store.claim(data['requestId'], dash._identity_key(identity), data)
    result = run_operation(live.store, data['requestId'], adapter)
    assert result.get('pending') is True and result.get('confirmed') is False, result
    row = live.store.get(data['requestId'])
    assert row['state'] == 'awaiting_confirmation'
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        row = dash.refresh_session_confirmation(live.store, row)
        if row['state'] == 'confirmed':
            break
        time.sleep(.1)
    assert row['state'] == 'confirmed', row['result']
    assert row['result']['observed']['harnessAccount'] == 'work'
