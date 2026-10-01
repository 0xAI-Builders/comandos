"""Puente tmux -> portapapeles del sistema.

Toda copia hecha DENTRO de tmux acaba en un buffer automático `bufferN`:
arrastre en copy-mode, doble/triple clic (en tmux 3.2 esos bindings no llaman a
xclip) y, sobre todo, las TUIs que capturan el mouse (Claude Code, Codex, Grok
en pantalla alternativa) y copian con OSC 52: tmux guarda el buffer y reenvía el
OSC 52 a la terminal, pero VTE no implementa OSC 52 y el texto nunca llega al
portapapeles del sistema. Este módulo detecta el buffer automático nuevo para que
la app lo publique.

Los buffers con nombre propio (`-b cc-type-…`) son internos: la app y cc-dash
los cargan para teclear en un pane y los borran enseguida. Nunca se publican.
"""
from __future__ import annotations
import re

AUTO = re.compile(r"^buffer(\d+)$")
MAX_BYTES = 8 * 1024 * 1024
LIST_FORMAT = "#{buffer_name}\t#{buffer_size}"


def newest_auto(listing):
    """(número, nombre, tamaño) del buffer automático más reciente de la salida de
    `tmux list-buffers -F LIST_FORMAT`, o None. tmux numera los automáticos en
    orden de creación, así que el número mayor es el último copiado."""
    best = None
    for line in (listing or "").splitlines():
        name, _, size = line.partition("\t")
        m = AUTO.match(name.strip())
        if not m:
            continue
        try:
            n, sz = int(m.group(1)), int(size or 0)
        except ValueError:
            continue
        if best is None or n > best[0]:
            best = (n, name.strip(), sz)
    return best


class Bridge:
    """Recuerda el último buffer publicado. El primer `poll` solo fija la marca:
    al abrir la app no se pisa lo que ya hay en el portapapeles."""

    def __init__(self):
        self.seen = None          # número del último buffer automático visto

    def poll(self, listing):
        """Nombre del buffer automático nuevo que hay que publicar, o None."""
        top = newest_auto(listing)
        if top is None:
            return None
        n, name, size = top
        if self.seen is None:
            self.seen = n
            return None
        if n <= self.seen:
            if n < self.seen:     # el servidor tmux se reinició: numeración desde cero
                self.seen = n
            return None
        self.seen = n
        return name if 0 < size <= MAX_BYTES else None
