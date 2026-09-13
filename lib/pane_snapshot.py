"""Identify the foreground CLI belonging to one tmux pane, without GTK or HTTP."""
from collections import defaultdict, deque
import json
import os
from pathlib import Path
import re

_ROLLOUT = re.compile(r'rollout-.*-([0-9a-f-]{36})\.jsonl$')
_SHELLS = {'zsh', 'bash', 'sh', 'fish', 'dash', 'ksh'}


def _json(path, default):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return default


class PaneInspector:
    """Take one process/account inventory, then inspect each pane independently."""
    def __init__(self, home=None, proc_root='/proc', flags_for_pid=None):
        self.home = Path(home or Path.home())
        self.proc = Path(proc_root)
        self.flags = flags_for_pid or self._flags
        self.children = defaultdict(list)
        self.claude = {}
        self.grok = {}
        for p in self.proc.iterdir():
            if not p.name.isdigit():
                continue
            try:
                parent = int((p / 'stat').read_text().rsplit(')', 1)[1].split()[1])
                self.children[parent].append(int(p.name))
            except (OSError, ValueError, IndexError):
                continue
        configs = [self.home / '.claude'] + list((self.home / '.claude-accounts').glob('*'))
        for cfg in configs:
            for f in (cfg / 'sessions').glob('*.json'):
                d = _json(f, {})
                if isinstance(d, dict) and d.get('pid') and d.get('sessionId'):
                    self.claude[d['pid']] = {
                        'agent': 'claude', 'resume_id': d['sessionId'],
                        'claude_config_dir': str(cfg),
                    }
        configs = [self.home / '.grok'] + list((self.home / '.grok-accounts').glob('*'))
        for cfg in configs:
            records = _json(cfg / 'active_sessions.json', [])
            for d in records if isinstance(records, list) else []:
                if isinstance(d, dict) and d.get('pid') and d.get('session_id'):
                    self.grok[d['pid']] = {
                        'agent': 'grok', 'resume_id': d['session_id'], 'grok_home': str(cfg),
                    }
        self.acp = _json(self.home / '.claude/hooks/acp-panes.json', {})

    def _flags(self, pid):
        try:
            args = [a.decode(errors='replace') for a in (self.proc / str(pid) / 'cmdline').read_bytes().split(b'\0') if a]
        except OSError:
            return []
        values = {'--model', '-m', '--effort', '--permission-mode', '--add-dir', '--settings', '--mcp-config'}
        switches = {'--dangerously-skip-permissions', '--dangerously-bypass-approvals-and-sandbox', '--allow-dangerously-skip-permissions'}
        out, i = [], 1
        while i < len(args):
            arg = args[i]
            if arg in values and i + 1 < len(args) and not args[i + 1].startswith('-'):
                out.extend(['--model' if arg == '-m' else arg, args[i + 1]])
                i += 2
            else:
                if arg in switches:
                    out.append(arg)
                i += 1
        return out

    def __call__(self, pane):
        if pane.get('command') in _SHELLS:
            return {'agent': ''}
        if pane.get('command') == 'cc-acp':
            record = self.acp.get(pane['id'], {})
            descendants, pending = set(), [int(pane['pid'])]
            while pending:
                process = pending.pop()
                if process in descendants:
                    continue
                descendants.add(process)
                pending.extend(self.children.get(process, []))
            if record.get('pid') not in descendants:
                return {'agent': 'acp'}
            return {'agent': 'acp', 'acp': {
                key: record.get(key, '') for key in ('agent', 'model', 'effort', 'account', 'sessionId')
            }}
        queue = deque([int(pane['pid'])])
        seen = set()
        while queue:
            pid = queue.popleft()
            if pid in seen:
                continue
            seen.add(pid)
            p = self.proc / str(pid)
            try:
                args = [a.decode(errors='replace') for a in (p / 'cmdline').read_bytes().split(b'\0') if a]
            except OSError:
                continue
            if not args:
                continue
            # Match a real CLI process before considering its subprocesses/subagents.
            exe = Path(args[0]).name
            if exe == 'claude':
                return {**self.claude[pid], 'flags': self.flags(pid)} if pid in self.claude else {'agent': 'claude'}
            if exe in ('grok', 'grok-linux-x86_64', 'grok-linux-aarch64'):
                return {**self.grok[pid], 'flags': self.flags(pid)} if pid in self.grok else {'agent': 'grok'}
            if exe == 'codex':
                sid = None
                candidates = []
                for fd in (p / 'fd').glob('*'):
                    try:
                        match = _ROLLOUT.search(os.readlink(fd))
                    except OSError:
                        continue
                    if match:
                        # One Codex process holds root AND delegated rollouts.
                        # Its first FD is not necessarily the visible conversation.
                        try:
                            with fd.open() as f:
                                meta = json.loads(f.readline(262144))
                            payload = meta.get('payload', {})
                            source = payload.get('source')
                            delegated = isinstance(source, dict) and 'subagent' in source
                            delegated = delegated or (isinstance(source, str) and source.startswith('subagent'))
                            if meta.get('type') == 'session_meta' and not delegated and payload.get('id') == match.group(1):
                                candidates.append((fd.stat().st_mtime_ns, match.group(1)))
                        except (OSError, ValueError, AttributeError):
                            continue
                if candidates:
                    sid = max(candidates)[1]
                if not sid and 'resume' in args:
                    i = args.index('resume') + 1
                    if i < len(args) and re.fullmatch(r'[0-9a-f-]{36}', args[i]):
                        sid = args[i]
                if sid:
                    result = {'agent': 'codex', 'resume_id': sid, 'flags': self.flags(pid)}
                    try:
                        env = dict(v.split(b'=', 1) for v in (p / 'environ').read_bytes().split(b'\0') if b'=' in v)
                        if env.get(b'CODEX_HOME'):
                            result['codex_home'] = os.fsdecode(env[b'CODEX_HOME'])
                    except OSError:
                        pass
                    return result
                # Never take a delegated Codex process's rollout as its parent's.
                return {'agent': 'codex'}
            queue.extend(self.children.get(pid, []))
        return {'agent': ''}


class CodexMetadataCache:
    """Bounded root-rollout metadata cache; idle panes perform stat only."""
    def __init__(self, max_entries=128, max_bytes=2_000_000, chunk_bytes=65536):
        from collections import OrderedDict
        import threading
        self.entries = OrderedDict()
        self.lock = threading.Lock()
        self.max_entries, self.max_bytes, self.chunk_bytes = max_entries, max_bytes, chunk_bytes

    @staticmethod
    def _signature(stat):
        return stat.st_dev, stat.st_ino, stat.st_size, stat.st_mtime_ns

    def read(self, pid, session_id, path):
        try:
            before = os.stat(path)
        except OSError:
            return {}
        key = (pid, session_id, *self._signature(before))
        with self.lock:
            if key in self.entries:
                self.entries.move_to_end(key)
                return dict(self.entries[key])
        try:
            result = self._read_context(path, before.st_size)
            if self._signature(os.stat(path)) == self._signature(before):
                with self.lock:
                    # Replace obsolete versions of the same process/conversation.
                    for old in [k for k in self.entries if k[:2] == key[:2]]:
                        self.entries.pop(old, None)
                    self.entries[key] = result
                    while len(self.entries) > self.max_entries:
                        self.entries.popitem(last=False)
            return dict(result)
        except OSError:
            return {}

    def _read_context(self, path, size):
        lower, end, prefix = max(0, size - self.max_bytes), size, b''
        with open(path, 'rb') as fh:
            while end > lower:
                start = max(lower, end - self.chunk_bytes)
                fh.seek(start)
                lines = (fh.read(end - start) + prefix).split(b'\n')
                prefix = lines.pop(0) if start else b''
                for line in reversed(lines):
                    try:
                        row = json.loads(line)
                    except (ValueError, UnicodeDecodeError):
                        continue
                    if isinstance(row, dict) and row.get('type') == 'turn_context':
                        context = row.get('payload') or {}
                        if not isinstance(context, dict):
                            continue
                        return {'model': context.get('model') or '',
                                'effort': context.get('effort') or context.get('reasoning_effort') or ''}
                end = start
        return {}
