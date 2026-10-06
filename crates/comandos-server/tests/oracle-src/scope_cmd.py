def scope_cmd(argv):
    """En Linux con systemd la sesion vive en su propio scope (sobrevive a
    reinicios de cc-dash); en macOS/otros va directo (tmux ya es un demonio)."""
    if shutil.which("systemd-run"):
        return ["systemd-run", "--user", "--scope", "--collect", "--quiet"] + argv
    return argv
