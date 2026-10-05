"""Persistent launch policy is exercised only against harmless fake CLIs."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]


def module(name):
    path = ROOT / 'lib' / (name + '.py')
    assert path.is_file(), 'missing persistent YOLO policy'
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
    return m


def test_only_explicit_yolo_changes_launch_permissions():
    m = module('codex_yolo_policy')
    for args in (['resume', 'exact'], ['--sandbox', 'workspace-write'], ['--', '--yolo'], ['-m', '--yolo']):
        assert m.normalize_args(args) == args
    for flag in ('--yolo', '--dangerously-bypass-approvals-and-sandbox'):
        args = m.normalize_args(['resume', 'exact', flag, 'continua'])
        assert args[:3] == ['resume', '--no-daemon', '--dangerously-bypass-approvals-and-sandbox']
        assert args[3:] == ['exact', 'continua']
        assert m.normalize_args(args) == args


@pytest.mark.parametrize('command', ['resume', 'fork'])
def test_interactive_subcommand_receives_its_own_yolo_options(command):
    m = module('codex_yolo_policy')
    args = m.normalize_args(['--model', 'chosen', '--no-daemon', '--yolo', command, 'exact', 'continua'])
    assert args == ['--model', 'chosen', command, '--no-daemon', m.YOLO, 'exact', 'continua']


def test_prompt_or_option_value_is_not_mistaken_for_a_subcommand():
    m = module('codex_yolo_policy')
    assert m.normalize_args(['--model', 'resume', '--yolo', '--', 'fork']) == [
        '--no-daemon', m.YOLO, '--model', 'resume', '--', 'fork']


def test_yolo_removes_conflicting_permissions_but_keeps_model_account_and_literal_prompt():
    m = module('codex_yolo_policy')
    args = m.normalize_args(['--yolo', '-s', 'workspace-write', '--ask-for-approval=on-request',
        '-c', 'sandbox_mode="read-only"', '-c', 'model_reasoning_effort="ultra"', '--approve-for-me',
        '-m', 'chosen', '--', '--sandbox', 'literal prompt'])
    assert 'workspace-write' not in args and '--ask-for-approval=on-request' not in args
    assert 'sandbox_mode="read-only"' not in args and '--approve-for-me' not in args
    assert 'model_reasoning_effort="ultra"' in args and 'chosen' in args
    assert args[-3:] == ['--', '--sandbox', 'literal prompt']
    with pytest.raises(ValueError, match='remot'):
        m.normalize_args(['--remote', 'unix://remote', '--yolo'])


def fake_codex(tmp_path):
    target = tmp_path / 'vendor/codex-real'
    target.parent.mkdir()
    target.write_text('#!/usr/bin/env python3\nimport os,json,sys\n'
        'if "--help" in sys.argv: print("--no-daemon --dangerously-bypass-approvals-and-sandbox");raise SystemExit(0)\n'
        'print(json.dumps({"args":sys.argv[1:],"account":os.environ.get("CODEX_HOME")}))\n')
    target.chmod(0o755)
    entry = tmp_path / 'package/bin/codex'; entry.parent.mkdir(parents=True)
    entry.symlink_to(target)
    return entry, target


def test_install_is_durable_idempotent_and_applies_to_an_existing_launcher_path(tmp_path):
    m = module('codex_yolo_install')
    home = tmp_path / 'home'; home.mkdir()
    rc = home / '.zshrc'; rc.write_text('# personal settings\n')
    entry, original = fake_codex(tmp_path)
    result = m.install(home=home, executable=entry)
    wrapper = home / '.local/bin/codex'
    assert wrapper.is_file() and os.access(wrapper, os.X_OK)
    assert entry.resolve() == wrapper.resolve()
    assert original.is_file()
    assert result['original'] == str(original)
    # Installed code keeps working without importing anything from the repo.
    env = dict(os.environ, CODEX_HOME='/tmp/personal-account')
    out = subprocess.run([str(entry), 'resume', 'exact', '--yolo', 'continua'], env=env,
                         capture_output=True, text=True, check=True)
    observed = json.loads(out.stdout)
    assert observed['account'] == '/tmp/personal-account'
    assert observed['args'] == ['resume', '--no-daemon', '--dangerously-bypass-approvals-and-sandbox', 'exact', 'continua']
    plain = subprocess.run([str(wrapper), '--version'], capture_output=True, text=True, check=True)
    assert json.loads(plain.stdout)['args'] == ['--version']
    before = rc.read_text()
    assert '# personal settings' in before
    again = m.install(home=home, executable=entry)
    assert again['original'] == str(original) and rc.read_text() == before
    assert before.count('# BEGIN COMANDOS CODEX YOLO') == 1
    assert Path(result['backup']).read_text() == '# personal settings\n'


def test_unknown_existing_wrapper_is_preserved_and_installation_stops(tmp_path):
    m = module('codex_yolo_install')
    home = tmp_path / 'home'; home.mkdir()
    entry, original = fake_codex(tmp_path)
    wrapper = home / '.local/bin/codex'; wrapper.parent.mkdir(parents=True)
    wrapper.write_text('#!/bin/sh\necho personal launcher\n'); wrapper.chmod(0o755)
    with pytest.raises(ValueError, match='exist'):
        m.install(home=home, executable=entry)
    assert entry.resolve() == original
    assert 'personal launcher' in wrapper.read_text()
    assert not (home / '.zshrc').exists()
