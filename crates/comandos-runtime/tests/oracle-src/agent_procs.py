def agent_procs():
    """Procesos de CUALQUIER agente corriendo: [(pid, cwd, agent)].
    Linux: /proc (cmdline, porque gemini/npm corren como 'node script.js').
    macOS (sin /proc): ps + lsof."""
    agents = agent_set()
    aliases = agent_process_aliases()
    out = []
    if os.path.isdir("/proc"):
        for p in glob.glob("/proc/[0-9]*/cmdline"):
            try:
                with open(p, "rb") as f:
                    argv = f.read().split(b"\0")
                names = [os.path.basename(a.decode("utf-8", "replace")) for a in argv[:3] if a]
                hit = next((aliases[name] for name in names if name in aliases), "")
                if hit:
                    d = os.path.dirname(p)
                    out.append((int(os.path.basename(d)), os.readlink(d + "/cwd"), hit))
            except Exception:
                pass
        return out
    # macOS / BSD
    try:
        r = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True,
                           text=True, timeout=5)
        cands = []
        for line in r.stdout.splitlines():
            parts = line.split(None, 1)
            if len(parts) != 2:
                continue
            pid, cmd = parts
            names = {os.path.basename(tok) for tok in cmd.split()[:2]}
            hit = names & agents
            if hit:
                cands.append((int(pid), sorted(hit)[0]))
        if cands:
            pids = ",".join(str(p) for p, _ in cands)
            lr = subprocess.run(["lsof", "-a", "-d", "cwd", "-p", pids, "-Fn"],
                                capture_output=True, text=True, timeout=8)
            cwds, cur = {}, None
            for l in lr.stdout.splitlines():
                if l.startswith("p"):
                    cur = int(l[1:])
                elif l.startswith("n") and cur is not None:
                    cwds[cur] = l[1:]
            for pid, ag in cands:
                if pid in cwds:
                    out.append((pid, cwds[pid], ag))
    except Exception:
        pass
    return out
