"""Ctrl+C en Grok o Codex: el ^C llega al CLI como siempre; lo que dejó
despegado se limpia solo si el CLI terminó.

Grok y Codex atrapan SIGINT y lanzan hijos con sesión o grupo propio (MCP,
herramientas, subagentes). El ^C del terminal solo alcanza al grupo en
primer plano, así que al salir el CLI esos hijos pueden quedar huérfanos.

Diseño, de lo más barato a lo más caro y sin tocar nunca al CLI:

1. Un Ctrl+C normal no hace nada extra. El ^C llega al CLI, que interrumpe
   su turno y restaura el terminal él mismo. Jamás se le manda SIGKILL.
2. Con selección (se copia), copy-mode de tmux, prefijo pulsado o una tabla
   de teclas distinta de root no se arma nada.
3. Un segundo Ctrl+C en menos de DOUBLE_TAP_S en el mismo terminal (el gesto
   con el que se sale de estos CLI) arma la limpieza en un hilo aparte:
   - el grupo en primer plano del tty del pane (tpgid) tiene que estar
     liderado por grok/codex. Un grok lanzado por otro programa
     (claude -> pytest -> grok, `codex exec` de Claude) o en segundo plano
     no cuenta;
   - se fotografían sus descendientes con OTRO grupo de procesos;
   - se espera hasta EXIT_WAIT_S a que el CLI salga por su cuenta. Si sigue
     vivo (solo se interrumpía el turno) no se toca nada;
   - si salió, a los despegados que siguen vivos con el mismo starttime:
     SIGINT y, tras GRACE_S, SIGTERM. Nunca SIGKILL.

Si un ^C se lo come un prompt de tmux (no hay formato para detectarlo en
3.2), el CLI no sale y el paso 3 tampoco hace nada.
"""
import os
import signal
import time

DOUBLE_TAP_S = 1.0
EXIT_WAIT_S = 5.0
GRACE_S = 1.0
_WRAPPERS = ("node", "bun", "deno")


def is_agent_name(name):
    name = os.path.basename(name or "")
    return name in ("grok", "codex") or name.startswith(("grok-", "codex-"))


def ctrl_c_action(has_selection, copy_context, now, last, window=DOUBLE_TAP_S):
    """copy | pass | arm. Solo 'arm' puede disparar la limpieza."""
    if has_selection:
        return "copy"
    if copy_context:
        return "pass"
    if last is not None and 0 <= now - last <= window:
        return "arm"
    return "pass"


def parse_client_state(output, tty):
    """Fila de `list-clients` del cliente de este terminal, o None."""
    for line in (output or "").splitlines():
        parts = line.split("|")
        if len(parts) != 5 or parts[0] != tty:
            continue
        _tty, table, prefix, in_mode, pid = parts
        return {"key_table": table, "prefix": prefix == "1",
                "in_mode": in_mode == "1",
                "pane_pid": int(pid) if pid.isdigit() else None}
    return None


def client_blocks_stop(state):
    """Copy-mode, prefijo o tabla de teclas ajena: el ^C no es para el CLI."""
    if not state or not state.get("pane_pid"):
        return True
    return bool(state.get("in_mode") or state.get("prefix")
                or state.get("key_table", "root") not in ("root", ""))


def descendant_pids(root, children):
    out = []
    stack = list(children.get(root, ()))
    seen = set()
    while stack:
        pid = stack.pop()
        if pid in seen or pid == root:
            continue
        seen.add(pid)
        out.append(pid)
        stack.extend(children.get(pid, ()))
    return out


def _children_index(processes):
    children = {}
    for proc in processes:
        children.setdefault(proc["ppid"], []).append(proc["pid"])
    return children


def _read_cmdline(pid):
    try:
        raw = open(f"/proc/{pid}/cmdline", "rb").read()
    except OSError:
        return []
    return [a.decode(errors="replace") for a in raw.split(b"\0") if a]


def _is_agent_proc(proc, cmdline):
    if is_agent_name(proc.get("comm")):
        return True
    argv = cmdline(proc["pid"])
    if argv and is_agent_name(argv[0]):
        return True
    # codex de npm: `node .../bin/codex`
    return bool(len(argv) > 1 and os.path.basename(argv[0]) in _WRAPPERS
                and is_agent_name(argv[1]))


def foreground_agent(pane_pid, processes, cmdline=_read_cmdline):
    """Líder del grupo en primer plano del pane, solo si ES grok/codex."""
    procs = {proc["pid"]: proc for proc in processes}
    pane = procs.get(pane_pid)
    if pane is None or pane.get("tpgid", -1) <= 0:
        return None
    leader = procs.get(pane["tpgid"])
    if leader is None or leader["pgid"] != leader["pid"]:
        return None
    if leader["pid"] != pane_pid and leader["pid"] not in descendant_pids(pane_pid, _children_index(procs.values())):
        return None
    return leader if _is_agent_proc(leader, cmdline) else None


def detached_descendants(agent, processes):
    """Descendientes del CLI fuera de su grupo: los que el ^C no alcanza."""
    procs = {proc["pid"]: proc for proc in processes}
    children = _children_index(procs.values())
    return [procs[pid] for pid in sorted(descendant_pids(agent["pid"], children))
            if procs[pid]["pgid"] != agent["pgid"]]


def _parse_stat(raw):
    marker = raw.rfind(b")")
    if marker < 0:
        return None
    rest = raw[marker + 2:].split()
    # tras el comm: 0 state, 1 ppid, 2 pgrp, 3 session, 4 tty_nr, 5 tpgid, 19 starttime
    if len(rest) < 20:
        return None
    try:
        return {"ppid": int(rest[1]), "pgid": int(rest[2]), "sid": int(rest[3]),
                "tpgid": int(rest[5]), "start": int(rest[19])}
    except ValueError:
        return None


def read_processes():
    """Foto de /proc: pid, ppid, pgid, sid, tpgid, comm y starttime."""
    found = []
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        pid = int(name)
        try:
            raw = open(f"/proc/{pid}/stat", "rb").read()
            status = open(f"/proc/{pid}/status", "r", errors="replace").read()
        except OSError:
            continue
        stat = _parse_stat(raw)
        if stat is None:
            continue
        comm = ""
        for line in status.splitlines():
            if line.startswith("Name:"):
                comm = line.split(":", 1)[1].strip()
                break
        found.append({"pid": pid, "comm": comm, **stat})
    return found


def still_same(records):
    """Los que siguen vivos con el mismo starttime (el pid no se reutilizó)."""
    alive = []
    for rec in records:
        try:
            raw = open(f"/proc/{rec['pid']}/stat", "rb").read()
        except OSError:
            continue
        stat = _parse_stat(raw)
        if stat is not None and stat["start"] == rec["start"]:
            alive.append(rec)
    return alive


def signal_records(records, sig, kill=None):
    kill = kill or os.kill
    sent = []
    for rec in records:
        try:
            kill(rec["pid"], sig)
        except (ProcessLookupError, PermissionError):
            continue
        sent.append(rec["pid"])
    return sent


def cleanup_after_exit(pane_pid, processes=None, *, alive=still_same, kill=None,
                       sleep=time.sleep, clock=time.monotonic,
                       wait_s=EXIT_WAIT_S, grace_s=GRACE_S, cmdline=_read_cmdline):
    """Corre en un hilo tras el doble Ctrl+C. Devuelve los pids señalados."""
    if processes is None:
        processes = read_processes()
    agent = foreground_agent(pane_pid, processes, cmdline)
    if agent is None:
        return []
    targets = detached_descendants(agent, processes)
    if not targets:
        return []
    deadline = clock() + wait_s
    while alive([agent]):
        if clock() >= deadline:
            return []   # el CLI sigue: solo se interrumpió el turno
        sleep(0.1)
    survivors = alive(targets)
    sent = signal_records(survivors, signal.SIGINT, kill)
    if not survivors:
        return sent
    sleep(grace_s)
    signal_records(alive(survivors), signal.SIGTERM, kill)
    return sent
