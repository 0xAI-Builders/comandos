"""Per-conversation extension drafts; shares the configuration coordinator's lock.

This store contains only selection IDs and booleans. Loaded state must come from
verified process evidence, never from a saved draft or an optimistic UI update.
"""
from __future__ import annotations

import hashlib
import json
import re
import time
import uuid

from session_operations import OperationStore


class ExtensionConflict(ValueError):
    pass


def selection_value(value):
    if not isinstance(value, dict) or set(value) != {'mcps', 'skills'}:
        raise ValueError('selección de extensiones inválida')
    result = {}
    for kind in ('mcps', 'skills'):
        rows = value[kind]
        if not isinstance(rows, dict) or len(rows) > 3000:
            raise ValueError('selección de extensiones inválida')
        if any(not isinstance(key, str) or not re.fullmatch(r'[A-Za-z0-9_.:@/+-]{1,256}', key)
               or type(enabled) is not bool for key, enabled in rows.items()):
            raise ValueError('identificador o estado de extensión inválido')
        result[kind] = dict(rows)
    return result


class ExtensionStore:
    def __init__(self, path):
        self.operations = OperationStore(path)
        self.path = self.operations.path
        with self.operations.connect() as db:
            db.executescript('''
                CREATE TABLE IF NOT EXISTS pane_extension_drafts (
                    key TEXT PRIMARY KEY, identity TEXT NOT NULL,
                    conversation TEXT NOT NULL, harness TEXT NOT NULL,
                    desired TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 0,
                    updated REAL NOT NULL);
                CREATE TABLE IF NOT EXISTS pane_extension_templates (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL,
                    selection TEXT NOT NULL, updated REAL NOT NULL);
            ''')

    @staticmethod
    def _row(row):
        if row is None:
            raise ExtensionConflict('el borrador ya no existe; vuelve a abrir el estante')
        result = dict(row)
        result['desired'] = json.loads(result['desired'])
        return result

    def state(self, identity, conversation, harness, defaults):
        defaults = selection_value(defaults)
        if not isinstance(identity, str) or not identity or len(identity) > 4096:
            raise ValueError('identidad de panel inválida')
        if not isinstance(conversation, str) or len(conversation) > 256:
            raise ValueError('conversación inválida')
        if harness not in ('claude', 'codex', 'grok', 'opencode', 'agy'):
            raise ValueError('CLI no compatible con el estante')
        key = hashlib.sha256(json.dumps([identity, conversation, harness]).encode()).hexdigest()
        with self.operations.connect() as db:
            db.execute('INSERT OR IGNORE INTO pane_extension_drafts '
                       '(key,identity,conversation,harness,desired,updated) VALUES (?,?,?,?,?,?)',
                       (key, identity, conversation, harness, json.dumps(defaults), time.time()))
            row = db.execute('SELECT * FROM pane_extension_drafts WHERE key=?', (key,)).fetchone()
        return self._row(row)

    def require_revision(self, key, revision):
        with self.operations.connect() as db:
            row = self._row(db.execute('SELECT * FROM pane_extension_drafts WHERE key=?', (key,)).fetchone())
        if type(revision) is not int or row['revision'] != revision:
            raise ExtensionConflict('la selección cambió en otro cliente; vuelve a cargarla')
        return row

    def save(self, key, revision, desired):
        desired = selection_value(desired)
        with self.operations.connect() as db:
            db.execute('BEGIN IMMEDIATE')
            row = self._row(db.execute('SELECT * FROM pane_extension_drafts WHERE key=?', (key,)).fetchone())
            if type(revision) is not int or row['revision'] != revision:
                raise ExtensionConflict('la selección cambió en otro cliente; vuelve a cargarla')
            active = db.execute("SELECT id FROM session_operations WHERE pane_key=? "
                                "AND state NOT IN ('confirmed','failed','rolled_back') LIMIT 1",
                                (row['identity'],)).fetchone()
            if active:
                raise ExtensionConflict('hay una operación en curso para este panel')
            db.execute('UPDATE pane_extension_drafts SET desired=?,revision=revision+1,updated=? WHERE key=?',
                       (json.dumps(desired), time.time(), key))
            result = db.execute('SELECT * FROM pane_extension_drafts WHERE key=?', (key,)).fetchone()
        return self._row(result)

    def templates(self):
        with self.operations.connect() as db:
            rows = db.execute('SELECT * FROM pane_extension_templates ORDER BY updated,id').fetchall()
        return [dict(row, selection=json.loads(row['selection'])) for row in rows]

    def save_template(self, name, selection):
        if not isinstance(name, str) or not 1 <= len(name.strip()) <= 80 or any(ord(c) < 32 for c in name):
            raise ValueError('nombre de plantilla inválido')
        selection = selection_value(selection)
        result = {'id': uuid.uuid4().hex, 'name': name.strip(), 'selection': selection, 'updated': time.time()}
        with self.operations.connect() as db:
            db.execute('INSERT INTO pane_extension_templates (id,name,selection,updated) VALUES (?,?,?,?)',
                       (result['id'], result['name'], json.dumps(selection), result['updated']))
        return result
