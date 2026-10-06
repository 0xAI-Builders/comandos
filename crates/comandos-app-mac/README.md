# AppKit consumer (M3)

This crate implements the M3 window, dashboard and `centro` bridge from the approved Mac plan. AppKit ownership and `unsafe` are confined to /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m3/crates/comandos-app-mac/src/ffi/. The other modules forbid unsafe Rust. Linux execution prints a macOS requirement and exits 2 without initializing a GUI.

Native sandbox launch requires a pre-existing owned 0700 directory, an explicit private loopback dashboard port in 7200–7399, and its authenticated terminal service:

```text
comandos-app-mac --sandbox /tmp/owned-comandos-mac --dash-url http://127.0.0.1:7340 --dump-dom /tmp/owned-comandos-mac/dashboard.html
```

The optional DOM destination must be absolute and absent; publication never replaces existing files. The sandbox tmux runner always uses its captured private `-S` socket. Preferences/configuration remain available when terminal authentication fails, and terminal actions stay queued until authenticated boot succeeds. HTTP/tmux work runs on a bounded cancellable worker; UI callbacks resolve weak owners on main and retain per-instance generations. GTK and Mac share the single Desktop process runner.

The complete 84-node original AST ledger and safety/lifecycle boundaries are documented in /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m3/docs/verification/mac-app-method-coverage.json and /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m3/docs/verification/mac-app-m3-boundaries.md. The immutable reference is /home/someguy/codebase/0xJesus/ComandOS/bin/cc-app-mac. M4 still owns native menus/dialogs, tab close/history/persistence, restoration and file IPC; these are not represented as complete in M3. Native link/GUI, RSS and one-hour acceptance require independent execution on the Mac.
