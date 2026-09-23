"""Ctrl+C en Grok o Codex tiene que parar el proceso y lo que haya despegado.

Esos CLI atrapan SIGINT y lanzan hijos con sesión propia (MCP, herramientas,
subagentes). El ^C del terminal solo toca el grupo en primer plano, así que
el resto sigue. Esto elige el árbol del agente —nunca el shell— para
señalarlo entero.
"""
import os
import signal

AGENTS = ("grok", "codex")


def _is_agent(comm):
    name = (comm or "").split("/")[-1]
    return name == "grok" or name == "codex" or name.startswith("codex-")


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


def agent_stop_records(root_pid, processes):
    """Pids del agente y sus hijos. Vacío si en ese pane no hay grok ni codex.

    No incluye el shell que tmux dejó como pane_pid, salvo que ese pid sea
    ya el agente. Sí incluye hijos con otro grupo de procesos.
    """
    procs = {proc["pid"]: proc for proc in processes}
    root = procs.get(root_pid)
    if root is None:
        return []
    children = _children_index(procs.values())
    agents = []
    if _is_agent(root["comm"]):
        agents.append(root_pid)
    for pid in descendant_pids(root_pid, children):
        if _is_agent(procs[pid]["comm"]):
            agents.append(pid)
    if not agents:
        return []
    chosen = set()
    for agent in agents:
        chosen.add(agent)
        chosen.update(descendant_pids(agent, children))
    return [procs[pid] for pid in sorted(chosen)]


def read_processes():
    """Foto de /proc: pid, ppid, pgid, comm y starttime."""
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
        marker = raw.rfind(b")")
        if marker < 0:
            continue
        rest = raw[marker + 2:].split()
        # field 22 del man proc es starttime; tras el comm, el índice es 19.
        if len(rest) < 20:
            continue
        comm = ""
        for line in status.splitlines():
            if line.startswith("Name:"):
                comm = line.split(":", 1)[1].strip()
                break
        try:
            found.append({
                "pid": pid,
                "ppid": int(rest[1]),
                "pgid": int(rest[2]),
                "comm": comm,
                "start": int(rest[19]),
            })
        except ValueError:
            continue
    return found


def signal_records(records, sig):
    sent = []
    for rec in records:
        try:
            os.kill(rec["pid"], sig)
        except (ProcessLookupError, PermissionError):
            continue
        sent.append(rec["pid"])
    return sent


def still_same(records):
    """Los que siguen vivos con el mismo starttime (el pid no se reutilizó)."""
    alive = []
    for rec in records:
        try:
            raw = open(f"/proc/{rec['pid']}/stat", "rb").read()
        except OSError:
            continue
        marker = raw.rfind(b")")
        if marker < 0:
            continue
        rest = raw[marker + 2:].split()
        if len(rest) > 19 and int(rest[19]) == rec["start"]:
            alive.append(rec)
    return alive


def interrupt_agent_tree(root_pid, processes=None):
    """SIGINT a todo el árbol del agente. Devuelve la foto para el SIGKILL posterior."""
    if processes is None:
        processes = read_processes()
    records = agent_stop_records(root_pid, processes)
    if records:
        signal_records(records, signal.SIGINT)
    return records


def kill_survivors(records):
    """SIGKILL a lo que seguía vivo después del SIGINT. Grok y Codex atrapan SIGTERM."""
    return signal_records(still_same(records), signal.SIGKILL)
