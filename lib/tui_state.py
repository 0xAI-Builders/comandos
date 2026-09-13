"""Read applied TUI configuration, without interpreting input as success.

Bounded caches hold only configuration metadata, never conversation text.
"""
from collections import OrderedDict
import json
import os
import re
import threading

EFFORT = r'(none|minimal|low|medium|high|xhigh|max|ultra|ultracode|auto)'
MODEL = r'(?:gpt-[a-z0-9.\-]+|claude-[a-z0-9.\-]+|grok-[a-z0-9.\-]+|(?:opus|sonnet|haiku|fable)(?:-[a-z0-9.]+)?)(?:\[1m\])?'
ANSI = re.compile(r'\x1b\[[0-?]*[ -/]*[@-~]')


def model_id(label):
    matches = re.findall(MODEL, label, re.I)
    # A concrete ID in parentheses is more useful than the preceding alias.
    if matches and any('-' in match for match in matches):
        return matches[-1].lower()
    display = re.search(r'\b(?:Claude\s+)?(Opus|Sonnet|Haiku|Fable)\s+(\d+(?:[.\-]\d+)*)', label, re.I)
    if display:
        return 'claude-' + display[1].lower() + '-' + display[2].replace('.', '-') + ('[1m]' if re.search(r'1M context|\[1m\]', label, re.I) else '')
    display = re.search(r'\bGrok\s+(\d+(?:\.\d+)*)', label, re.I)
    return 'grok-' + display[1] if display else (matches[-1].lower() if matches else '')


def screen_state(harness, text):
    lines = ANSI.sub('', text).splitlines()
    out = {}
    if harness == 'codex':
        # Only the dedicated footer; numbered options and prose are not state.
        for line in lines[-8:]:
            hit = re.fullmatch(r'\s*(' + MODEL + r')\s+' + EFFORT + r'\s*[·•].*', line, re.I)
            if hit:
                out = dict(model=hit[1].lower(), effort=hit[2].lower(), kind='status')
        return out
    if harness == 'opencode':
        # 1.17.18 full-screen composer: border + agent + model/provider + variant.
        for index, line in enumerate(lines[:-1]):
            match = re.match(r'^\s*┃\s+[\w-]+\s+·\s+(.+)$', line)
            if match and lines[index + 1].lstrip().startswith('╹▀'):
                model = model_id(match[1])
                if model:
                    effort = re.search(r'[·•]\s*' + EFFORT + r'\s*$', match[1], re.I)
                    out = dict(model=model, effort=effort[1].lower() if effort else '', kind='status')
        return out
    if harness == 'claude':
        for line in lines:
            if re.match(r'^\s*●\s', line):
                out = {}  # A later response makes historical command output stale.
            hit = re.match(r'^\s*⎿\s+(?:Set model to|Kept model as|Current model:)\s+(.+)', line)
            if hit and model_id(hit[1]):
                out = dict(model=model_id(hit[1]), kind='confirmation')
                combined = re.search(r'\b(?:with|and)\s+' + EFFORT + r'\s+effort\b', hit[1], re.I)
                if combined:
                    out['effort'] = combined[1].lower()
            effort = re.match(r'^\s*⎿\s+(?:Set effort level to|Effort(?: level)? set to)\s+' + EFFORT + r'\b', line, re.I)
            if effort and 'but' not in line.lower() and 'not applied' not in line.lower():
                out.update(effort=effort[1].lower(), kind='confirmation')
        for line in lines[-5:]:
            status = re.search(r'[·•]\s*claude\s*[·•]\s*(' + MODEL + r')', line, re.I)
            if status:
                # Native confirmations may be newer than a custom status script.
                if not out:
                    out = dict(model=status[1].lower(), kind='status')
        return out
    return out


class TranscriptCache:
    """Stat-only on idle; read at most 2 MiB after a transcript changes."""
    def __init__(self, max_entries=128, max_bytes=2097152):
        self.entries = OrderedDict()
        self.lock = threading.Lock()
        self.max_entries, self.max_bytes = max_entries, max_bytes

    def read(self, harness, session_id, path):
        try:
            st = os.stat(path)
            signature = (st.st_dev, st.st_ino, st.st_size, st.st_mtime_ns)
            key = (harness, session_id, str(path))
            with self.lock:
                previous = self.entries.get(key)
                if previous and previous[0] == signature:
                    self.entries.move_to_end(key)
                    return dict(previous[1])
            with open(path, 'rb') as handle:
                start = max(0, st.st_size - self.max_bytes)
                handle.seek(start)
                raw = handle.read(self.max_bytes)
            lines = raw.splitlines()[1:] if start else raw.splitlines()
            result = {}
            for index, line in enumerate(lines):
                try:
                    row = json.loads(line)
                except (ValueError, UnicodeDecodeError):
                    continue
                if not isinstance(row, dict) or row.get('isSidechain') or row.get('parent_session_id'):
                    continue
                sid = row.get('sessionId') or row.get('session_id')
                if sid and sid != session_id:
                    continue
                value = {}
                if harness == 'claude' and row.get('type') == 'assistant':
                    msg = row.get('message') or {}
                    if isinstance(msg, dict) and msg.get('model') and msg['model'] != '<synthetic>':
                        value = dict(model=msg['model'], effort=row.get('effort') or row.get('perTurnEffort') or '')
                elif harness == 'claude' and row.get('type') == 'user':
                    content = (row.get('message') or {}).get('content')
                    # Claude persists local command OUTPUT in a tagged user row.
                    if isinstance(content, str) and content.startswith('<local-command-stdout>'):
                        output = content.removeprefix('<local-command-stdout>').split('</local-command-stdout>')[0]
                        value = screen_state('claude', '⎿ ' + output)
                elif harness == 'codex' and row.get('type') == 'turn_context':
                    payload = row.get('payload') or {}
                    if isinstance(payload, dict) and payload.get('model'):
                        value = dict(model=payload['model'], effort=payload.get('effort') or payload.get('reasoning_effort') or '')
                if value:
                    if value.get('model') and value['model'] != result.get('model'):
                        result.pop('effort', None)
                    result.update(value, revision=row.get('uuid') or row.get('timestamp') or f'{signature}:{index}')
            # A huge tool output can push the last state beyond the bounded tail.
            if not result and previous and signature[:2] == previous[0][:2] and st.st_size >= previous[0][2]:
                result = dict(previous[1])
            with self.lock:
                self.entries[key] = (signature, result)
                self.entries.move_to_end(key)
                while len(self.entries) > self.max_entries:
                    self.entries.popitem(last=False)
            return dict(result)
        except OSError:
            return {}


class StateTracker:
    """Changed evidence wins; an unchanged old footer cannot undo a new turn."""
    def __init__(self, max_entries=128):
        self.entries = OrderedDict()
        self.lock = threading.Lock()
        self.max_entries = max_entries

    def observe(self, identity, launch, conversation, visible, *, now):
        with self.lock:
            entry = self.entries.get(identity)
            if entry is None:
                entry = {'value': dict(model=launch.get('model') or '', effort=launch.get('effort') or '',
                                       source='process' if launch.get('model') else 'unconfirmed')}
            value = entry['value']
            for name, evidence in [('conversation', conversation), ('pane', visible)]:
                fingerprint = tuple((key, evidence.get(key)) for key in ('model', 'effort', 'revision', 'kind'))
                if not evidence:
                    if name == 'pane':
                        entry.pop(name, None)
                    continue
                if fingerprint == entry.get(name):
                    continue
                entry[name] = fingerprint
                if evidence.get('model') and evidence['model'] != value.get('model'):
                    value['effort'] = ''
                for field in ('model', 'effort'):
                    if field in evidence:
                        value[field] = evidence[field]
                value.update(source=name, evidenceAt=now)
            self.entries[identity] = entry
            self.entries.move_to_end(identity)
            while len(self.entries) > self.max_entries:
                self.entries.popitem(last=False)
            return dict(value)


class GrokMetadataCache:
    """Resolve a PID's exact session once; stat its small summary on each poll."""
    def __init__(self, max_entries=128):
        self.entries, self.paths = OrderedDict(), OrderedDict()
        self.lock = threading.RLock()
        self.max_entries = max_entries

    def _json(self, path, project):
        try:
            st = os.stat(path)
            signature = (st.st_dev, st.st_ino, st.st_size, st.st_mtime_ns)
            hit = self.entries.get(path)
            if hit and hit[0] == signature:
                self.entries.move_to_end(path)
                return hit[1]
            with open(path) as handle:
                value = project(json.load(handle))
            self.entries[path] = (signature, value)
            while len(self.entries) > self.max_entries:
                self.entries.popitem(last=False)
            return value
        except (OSError, ValueError, TypeError, AttributeError):
            return {}

    def read(self, pid, home):
        import glob
        import grok_state
        with self.lock:
            active = self._json(os.path.join(home, 'active_sessions.json'),
                                lambda rows: {int(r.get('pid') or 0): r.get('session_id') for r in rows if isinstance(r, dict)})
            sid = active.get(int(pid))
            if not sid or not re.fullmatch(r'[A-Za-z0-9_-]+', str(sid)):
                return {}
            key = (home, sid)
            path = self.paths.get(key)
            if not path or not os.path.isfile(path):
                hits = glob.glob(os.path.join(home, 'sessions', '**', sid, 'summary.json'), recursive=True)
                if len(hits) != 1:
                    return {}
                path = hits[0]
                self.paths[key] = path
                while len(self.paths) > self.max_entries:
                    self.paths.popitem(last=False)
            self.paths.move_to_end(key)
            value = self._json(path, grok_state._summary_public)
            return dict(value, sessionId=sid, home=home)
