"""Bounded read-only counts from one exact conversation, with honest coverage.

No prompts, arguments or tool results leave this module. Incomplete sources retain
positive observations but never turn a missing observation into a zero.
"""
from collections import OrderedDict
from copy import deepcopy
import json
from pathlib import Path
import re
import sqlite3
import threading

_CACHE = OrderedDict()
_LOCK = threading.Lock()


def _signature(path):
    try:
        stat = path.stat()
        return stat.st_ino, stat.st_size, stat.st_mtime_ns
    except OSError:
        return None


def _arguments(value):
    if isinstance(value, dict):
        return value
    if isinstance(value, str) and len(value) < 262144:
        try:
            result = json.loads(value)
            return result if isinstance(result, dict) else {}
        except ValueError:
            pass
    return {}


def conversation_usage(harness, conversation_id, source, inventory, *, max_bytes=2_000_000):
    """Source must be resolved by the caller from verified process/conversation ID."""
    rows = {kind: inventory.get(kind) or [] for kind in ('mcps', 'skills')}
    result = {'counts': {kind: {r['id']: None for r in group} for kind, group in rows.items()},
              'complete': False, 'turns': 0, 'source': 'conversation', 'tokens': None,
              'note': 'Sin historial completo; la ausencia de llamadas no demuestra falta de uso.'}
    if not conversation_id or not source or harness not in ('claude', 'codex', 'grok', 'opencode', 'agy'):
        return result
    path = Path(source)
    signature = _signature(path)
    if signature is None:
        return result
    row_key = tuple((k, r['id'], r.get('name', ''), r.get('path', '')) for k, group in rows.items() for r in group)
    key = (harness, conversation_id, str(path), signature, _signature(Path(str(path)+'-wal')),
           row_key, max_bytes)
    with _LOCK:
        if key in _CACHE:
            _CACHE.move_to_end(key)
            return deepcopy(_CACHE[key])
    counts, seen = {}, set()
    complete = harness in ('claude', 'opencode')
    turns, malformed = 0, False
    server_names = sorted((r['name'] for r in rows['mcps']), key=len, reverse=True)

    def call(name, args=None, ident=None, server=None):
        nonlocal complete
        if not server and (not isinstance(name, str) or not name):
            complete = False
            return
        args = _arguments(args)
        kind, target = '', ''
        if server:
            kind, target = 'mcps', server
        elif isinstance(name, str) and name.startswith('mcp__') and len(name.split('__')) >= 3:
            kind, target = 'mcps', name.split('__', 2)[1]
        elif isinstance(name, str) and name.startswith('mcp__'):
            complete = False
            return
        elif name in ('Skill', 'skill'):
            kind, target = 'skills', args.get('skill') or args.get('name') or ''
        elif name == 'call_mcp_tool':
            kind, target = 'mcps', args.get('ServerName') or ''
        elif name in ('view_file', 'read_file'):
            filename = args.get('AbsolutePath') or args.get('path') or args.get('file_path')
            matches = [r for r in rows['skills'] if filename and filename == r.get('path')]
            if len(matches) == 1:
                kind, target = 'skills', matches[0]['name']
        elif harness == 'opencode' and isinstance(name, str):
            matches = [s for s in server_names if name.startswith(re.sub(r'[^A-Za-z0-9_-]', '_', s) + '_')]
            if len(matches) > 1:
                complete = False
                return
            target = matches[0] if matches else ''
            kind = 'mcps' if target else ''
        if not kind or not isinstance(target, str) or not target:
            if kind:
                complete = False
            return
        if len([r for r in rows[kind] if r['name'] == target]) > 1:
            complete = False
            return
        dedup = (kind, target, str(ident)) if ident is not None else None
        if dedup and dedup in seen:
            return
        if dedup:
            seen.add(dedup)
        counts[(kind, target)] = counts.get((kind, target), 0) + 1

    try:
        if harness == 'opencode':
            # The caller supplies the shared DB; the WHERE always pins the exact session.
            with sqlite3.connect(path.resolve().as_uri()+'?mode=ro', uri=True, timeout=.5) as db:
                records = db.execute('SELECT id,substr(data,1,?) FROM part WHERE session_id=? LIMIT 10001',
                                     (max_bytes + 1, conversation_id))
                consumed = 0
                for index, (ident, raw) in enumerate(records):
                    consumed += len(raw.encode('utf-8'))
                    if index >= 10000 or consumed > max_bytes:
                        complete = False
                        break
                    turns = 1  # A recorded part proves this conversation has begun.
                    r = json.loads(raw)
                    if r.get('type') == 'tool':
                        state = r.get('state') or {}
                        if state.get('status') not in ('running', 'completed', 'error'):
                            complete = False
                            continue
                        call(r.get('tool'), state.get('input'), r.get('callID') or ident)
        else:
            if signature[1] > max_bytes:
                complete = False
            with path.open('rb') as f:
                if signature[1] > max_bytes:
                    f.seek(-max_bytes, 2)
                    f.readline()  # Skip an incomplete initial JSONL record.
                raw = f.read(max_bytes + 1)
            if len(raw) > max_bytes:
                complete = False
                raw = raw[:max_bytes]
            for index, line in enumerate(raw.splitlines()):
                try:
                    r = json.loads(line)
                except (ValueError, UnicodeDecodeError):
                    malformed = True
                    continue
                if not isinstance(r, dict):
                    malformed = True
                    continue
                if r.get('sessionId') and r['sessionId'] != conversation_id:
                    continue
                if harness == 'claude':
                    if r.get('type') == 'user':
                        turns += 1
                    if r.get('type') != 'assistant':
                        continue
                    for block in (r.get('message') or {}).get('content') or []:
                        if isinstance(block, dict) and block.get('type') == 'tool_use':
                            call(block.get('name'), block.get('input'), block.get('id') or f'line-{index}')
                elif harness == 'codex':
                    payload = r.get('payload') or {}
                    if r.get('type') == 'session_meta' and payload.get('id') != conversation_id:
                        return result
                    if r.get('type') == 'response_item' and payload.get('type') in ('function_call', 'custom_tool_call'):
                        call(payload.get('name'), payload.get('arguments'), payload.get('call_id') or f'line-{index}')
                    if payload.get('type') == 'user_message':
                        turns += 1
                elif harness == 'agy':
                    for offset, item in enumerate(r.get('tool_calls') or []):
                        if isinstance(item, dict):
                            call(item.get('name'), item.get('args'), item.get('id') or f'{r.get("step_index",index)}-{offset}')
                    if r.get('source') == 'USER':
                        turns += 1
                elif harness == 'grok':
                    payload = r.get('data') or r.get('event') or r
                    if not isinstance(payload, dict):
                        continue
                    typ = r.get('type') or payload.get('type')
                    if typ == 'mcp_tool_call_completed':
                        call(payload.get('tool_name'), ident=payload.get('tool_call_id') or f'line-{index}',
                             server=payload.get('server_name') or payload.get('server'))
                    elif typ in ('tool_call', 'tool_call_completed'):
                        call(payload.get('name') or payload.get('tool_name'), payload.get('arguments'),
                             payload.get('tool_call_id') or f'line-{index}')
                    if typ == 'user_message':
                        turns += 1
    except (OSError, ValueError, TypeError, AttributeError, sqlite3.Error):
        complete = False
        malformed = True
    complete = bool(complete and not malformed and turns)
    for kind, group in rows.items():
        for row in group:
            result['counts'][kind][row['id']] = counts.get((kind, row['name']), 0 if complete else None)
    result.update(complete=complete, turns=turns)
    if complete:
        result['note'] = 'Llamadas registradas en esta conversación.'
    with _LOCK:
        _CACHE[key] = deepcopy(result)
        while len(_CACHE) > 32:
            _CACHE.popitem(last=False)
    return result
