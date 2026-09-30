"""Teclea texto literal en un pane de tmux, letra por letra, sin Enter jamás."""
from __future__ import annotations
import threading, time

MAX_CHARS = 2000


class TypingError(RuntimeError):
    def __init__(self, message, *, typed=0, code="invalid"):
        super().__init__(message)
        self.typed, self.code = typed, code


def validate(text):
    if not isinstance(text, str) or not text.strip():
        raise TypingError("Texto vacío")
    if len(text) > MAX_CHARS:
        raise TypingError(f"Texto demasiado largo (máx. {MAX_CHARS})")
    if any(ch in "\r\n" or (ord(ch) < 32 and ch != "\t") or ord(ch) == 127 for ch in text):
        raise TypingError("El texto no puede contener saltos de línea ni caracteres de control")
    return text


def type_literal(tmux_fn, pane, text, *, sleep=time.sleep, delay=0.022, budget=1.2):
    text = validate(text)
    step = min(delay, budget / max(len(text) - 1, 1))
    typed = 0
    for i, ch in enumerate(text):
        r = tmux_fn("send-keys", "-t", pane, "-l", "--", ch)
        if getattr(r, "returncode", 1) != 0:
            raise TypingError((getattr(r, "stderr", "") or "tmux send-keys falló").strip(), typed=typed, code="tmux")
        typed += 1
        if i < len(text) - 1:
            sleep(step)
    return {"typed": typed}


class PaneTypingLocks:
    def __init__(self):
        self._busy, self._guard = set(), threading.Lock()

    def acquire(self, pane):
        with self._guard:
            if pane in self._busy:
                return False
            self._busy.add(pane)
            return True

    def release(self, pane):
        with self._guard:
            self._busy.discard(pane)
