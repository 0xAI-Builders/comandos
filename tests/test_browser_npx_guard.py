import json
import os
from pathlib import Path
import subprocess

import pytest

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'bin/cc-browser-npx-guard'
ORIGINAL_NODE = '/home/someguy/.nvm/versions/node/v22.19.0/bin/node'
ORIGINAL_NPX = '/home/someguy/.nvm/versions/node/v22.19.0/lib/node_modules/npm/bin/npx-cli.js'


@pytest.fixture
def guard(tmp_path):
    assert SOURCE.exists(), 'npx compatibility guard missing'
    local_bin = tmp_path / '.local/bin'
    local_bin.mkdir(parents=True)
    recorder = '#!/usr/bin/env python3\nimport json, sys\nprint(json.dumps({"program": sys.argv[0], "args": sys.argv[1:]}))\n'
    remote = local_bin / 'cc-browser-remote'
    remote.write_text(recorder)
    remote.chmod(0o755)
    node = tmp_path / 'original-node'
    node.write_text(recorder)
    node.chmod(0o755)
    script = tmp_path / 'npx'
    source = SOURCE.read_text()
    assert ORIGINAL_NODE in source and ORIGINAL_NPX in source
    script.write_text(source.replace(ORIGINAL_NODE, str(node)))
    script.chmod(0o755)

    def run(*args):
        result = subprocess.run([str(script), *args], env={**os.environ, 'HOME': str(tmp_path)}, capture_output=True, text=True)
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)

    return run, remote, node


@pytest.mark.parametrize('args', [
    ('chrome-devtools-mcp',),
    ('chrome-devtools-mcp@1.9.0', '--browserUrl', 'http://127.0.0.1:9222'),
    ('-y', 'chrome-devtools-mcp@latest', '--headless'),
    ('--yes', '--no-install', 'chrome-devtools-mcp@1.9.0', '--user-data-dir=/old'),
    ('--no-install', '-y', 'chrome-devtools-mcp', '--remote-debugging-port=0'),
])
def test_exact_first_package_routes_to_remote_without_node_or_legacy_flags(guard, args):
    run, remote, _ = guard
    assert run(*args) == {'program': str(remote), 'args': []}


@pytest.mark.parametrize('args', [
    (), ('--version',), ('--help',), ('-y',),
    ('other-package', 'chrome-devtools-mcp'),
    ('-y', 'other-package', 'chrome-devtools-mcp@latest'),
    ('--', 'chrome-devtools-mcp'),
    ('-y', '--', 'chrome-devtools-mcp'),
    ('--package', 'chrome-devtools-mcp', 'other-command'),
    ('-p', 'chrome-devtools-mcp', 'other-command'),
    ('--yes=true', 'chrome-devtools-mcp'),
    ('chrome-devtools-mcp-extra',),
    ('@scope/chrome-devtools-mcp',),
    ('./chrome-devtools-mcp',),
    ('other', 'a b', '$(touch /tmp/nope)', '', '--flag=value'),
])
def test_all_other_commands_preserve_exact_original_arguments(guard, args):
    run, _, node = guard
    assert run(*args) == {'program': str(node), 'args': [ORIGINAL_NPX, *args]}


def test_remote_failure_is_returned_without_local_fallback(guard):
    run, remote, _ = guard
    remote.write_text('#!/bin/sh\nexit 42\n')
    script = remote.parents[2] / 'npx'
    result = subprocess.run([str(script), '-y', 'chrome-devtools-mcp@1.9.0'], env={**os.environ, 'HOME': str(remote.parents[2])}, capture_output=True, text=True)
    assert result.returncode == 42
    assert result.stdout == ''
