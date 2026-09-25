"""Recorded provenance and reference token sizes, never inferred runtime usage."""
from functools import lru_cache
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

TOKENIZER = 'cl100k_base'
ENCODING_URL = 'https://openaipublic.blob.core.windows.net/encodings/cl100k_base.tiktoken'
ENCODING_HASH = '223921b76ee99bde995b7ff738513eef100fb51d18c93597a113bcffe865b2a7'
MAX_BYTES = 2_000_000


def _json(path):
    try:
        if path.stat().st_size > MAX_BYTES:
            return {}
        value = json.loads(path.read_text())
        return value if isinstance(value, dict) else {}
    except (OSError, ValueError, UnicodeError):
        return {}


def unknown_size(basis):
    return {'tokens': None, 'tokenizer': TOKENIZER, 'basis': basis}


def skill_origin(row, home):
    if row.get('plugin'):
        return {'id':'plugin:'+row['plugin'], 'label':row['plugin'], 'kind':'plugin'}
    try:
        path = Path(row['path']).resolve()
        root = (Path(home)/'.agents/skills').resolve()
        relative = path.relative_to(root)
        if len(relative.parts) == 2 and relative.name == 'SKILL.md':
            entries = _json(Path(home)/'.agents/.skill-lock.json').get('skills', {})
            record = entries.get(relative.parts[0], {}) if isinstance(entries, dict) else {}
            if not isinstance(record, dict):record = {}
            source = record.get('source')
            if record.get('sourceType') == 'github' and isinstance(source, str) and re.fullmatch(r'[\w.-]+/[\w.-]+', source):
                return {'id':'github:'+source, 'label':source, 'kind':'repository'}
    except (KeyError, OSError, ValueError, RuntimeError, TypeError):
        pass
    return {'id':'unknown', 'label':'Origen no registrado', 'kind':'unknown'}


def _cache_dir():
    return Path(os.environ.get('TIKTOKEN_CACHE_DIR') or Path.home()/'.cache/comandos/tiktoken')


def _offline_counts(texts):
    # Never download encoding data on an inventory request. Installation warms it.
    cache = _cache_dir()
    path = cache/hashlib.sha1(ENCODING_URL.encode()).hexdigest()
    if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != ENCODING_HASH:
        return [None]*len(texts)
    os.environ['TIKTOKEN_CACHE_DIR'] = str(cache)
    import tiktoken
    encoding = tiktoken.get_encoding(TOKENIZER)
    return [len(encoding.encode(text, disallowed_special=())) for text in texts]


def token_counts(texts):
    if not texts:
        return []
    try:
        if importlib.util.find_spec('tiktoken'):
            return _offline_counts(texts)
        python = Path.home()/'.local/share/comandos/extensions-venv/bin/python'
        if python.exists():
            result = subprocess.run([str(python), '-I', str(Path(__file__).resolve()), '--count'],
                                    input=json.dumps(texts), text=True, capture_output=True, timeout=8)
            counts = json.loads(result.stdout)
            if result.returncode == 0 and isinstance(counts,list) and len(counts) == len(texts) and all(n is None or type(n) is int and n >= 0 for n in counts):
                return counts
    except (ImportError, OSError, ValueError, subprocess.SubprocessError):
        pass
    return [None]*len(texts)


@lru_cache(maxsize=1024)
def _skill_text(path, mtime, size, inode):
    if size > MAX_BYTES:
        return None
    try:
        with open(path, encoding='utf-8') as stream:
            text = stream.read(MAX_BYTES+1)
        return text if len(text.encode()) <= MAX_BYTES else None
    except (OSError, UnicodeError):
        return None


_SIZE_CACHE = {}


def skill_metadata(rows, home):
    result, pending, texts = {}, [], []
    for row in rows:
        item = {'origin':skill_origin(row, home), 'size':unknown_size('skill-file')}
        result[row['id']] = item
        if row.get('group') or not row.get('path'):
            continue
        try:
            path = Path(row['path']).resolve()
            stat = path.stat()
            text = _skill_text(str(path), stat.st_mtime_ns, stat.st_size, stat.st_ino)
        except (OSError, ValueError, RuntimeError):
            continue
        if text is None:
            continue
        digest = hashlib.sha256(text.encode()).hexdigest()
        cached = _SIZE_CACHE.get(digest)
        if cached and (cached[0] is not None or time.monotonic()-cached[1] < 30):
            item['size']['tokens'] = cached[0]
        elif sum(len(t) for t in texts)+len(text) <= 8_000_000:
            pending.append((item,digest)); texts.append(text)
    for (item,digest), count in zip(pending, token_counts(texts)):
        item['size']['tokens'] = count
        if len(_SIZE_CACHE) >= 1024:
            _SIZE_CACHE.pop(next(iter(_SIZE_CACHE)), None)
        _SIZE_CACHE[digest] = (count,time.monotonic())
    return result


def _digest(value):
    return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':')).encode()).hexdigest()


def _size_path(home, name):
    return Path(home)/'.local/state/comandos/extensions/sizes'/(_digest(name)+'.json')


def record_mcp_size(home, name, spec, tools):
    """Persist only a count and hashes from an already-requested complete tool list."""
    definitions = sorted(({k:t[k] for k in ('name','description','inputSchema','outputSchema') if k in t} for t in tools), key=lambda t:t['name'])
    text = json.dumps(definitions,sort_keys=True,separators=(',',':'),ensure_ascii=False)
    if len(text.encode()) > MAX_BYTES:
        return
    count = token_counts([text])[0]
    if count is None:
        return
    data = {**unknown_size('tool-definitions'), 'tokens':count, 'configuration':_digest(spec), 'content':_digest(definitions), 'measuredAt':int(time.time())}
    path = _size_path(home,name)
    path.parent.mkdir(parents=True,exist_ok=True,mode=0o700)
    fd,tmp = tempfile.mkstemp(dir=path.parent,prefix='.size-')
    try:
        with os.fdopen(fd,'w') as f:
            os.fchmod(f.fileno(),0o600);json.dump(data,f)
        os.replace(tmp,path)
    finally:
        if os.path.exists(tmp):os.unlink(tmp)


def mcp_size(home, name, spec):
    value = _json(_size_path(home,name))
    measured = value.get('measuredAt')
    if (value.get('basis') == 'tool-definitions' and type(measured) is int
            and value.get('configuration') == _digest(spec) and value.get('tokenizer') == TOKENIZER
            and type(value.get('tokens')) is int and value['tokens'] >= 0
            and 0 <= time.time()-measured < 86400):
        return {k:value[k] for k in ('tokens','tokenizer','basis','measuredAt')}
    return unknown_size('tool-definitions')


if __name__ == '__main__':
    if sys.argv[1:] == ['--warm']:
        os.environ['TIKTOKEN_CACHE_DIR'] = str(_cache_dir())
        import tiktoken
        tiktoken.get_encoding(TOKENIZER)
    elif sys.argv[1:] == ['--count']:
        try:
            texts = json.loads(sys.stdin.read(16_000_001))
            print(json.dumps(_offline_counts(texts)))
        except Exception:
            print('[]')


class ToolListCapture:
    """Only complete, ordered pagination can produce a schema measurement."""
    def __init__(self):
        self.expected = None
        self.rows = None
        self.bytes = 0

    def add(self, cursor, next_cursor, tools):
        if cursor is None:
            self.rows, self.bytes = [], 0
        elif self.rows is None or cursor != self.expected:
            self.rows = None
            return None
        self.bytes += len(json.dumps(tools))
        if self.bytes > MAX_BYTES or len(self.rows)+len(tools) > 10000:
            self.rows = None
            return None
        self.rows.extend(tools)
        self.expected = next_cursor
        if next_cursor:
            return None
        result, self.rows = self.rows, None
        return result
