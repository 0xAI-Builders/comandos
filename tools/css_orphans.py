#!/usr/bin/env python3
"""Lista las clases e ids del <style> de dash/index.html que ya nadie usa en dash/.

Uso: python3 tools/css_orphans.py [prefijo ...]
Sin prefijos lista todo; con prefijos (p. ej. .uw- .qh- #usage-) solo esos.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
html = (ROOT / "dash" / "index.html").read_text(encoding="utf-8")
style = "\n".join(re.findall(r"<style>(.*?)</style>", html, flags=re.S))
rest = re.sub(r"<style>.*?</style>", "", html, flags=re.S)
for path in sorted((ROOT / "dash").glob("*.js")):
    rest += path.read_text(encoding="utf-8")
names = sorted(set(re.findall(r"([.#][A-Za-z][\w-]*)", re.sub(r"\{[^{}]*\}", "{}", style))))
wanted = sys.argv[1:]
for name in names:
    if wanted and not any(name.startswith(p) for p in wanted):
        continue
    bare = name[1:]
    if not re.search(r"(?<![\w-])" + re.escape(bare) + r"(?![\w-])", rest):
        print(name)
