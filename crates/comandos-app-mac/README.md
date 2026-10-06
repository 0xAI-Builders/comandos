# AppKit consumer (M3 + M4)

The crate owns the native window/dashboard/bridge and the M4 terminal menus, dialogs, restoration, history, persistence and file IPC. Toolkit ownership and unsafe Rust stay inside /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m4/crates/comandos-app-mac/src/ffi/. Other modules forbid unsafe Rust. Linux execution exits 2 without initializing a GUI.

Native sandbox launch requires an existing owned 0700 directory, a private loopback dashboard port in 7200–7399 and its authenticated terminal service:

```text
comandos-app-mac --sandbox /tmp/owned-comandos-mac --dash-url http://127.0.0.1:7340 --dump-dom /tmp/owned-comandos-mac/dashboard.html
```

The optional absolute DOM destination must be absent. The sandbox tmux runner captures its private `-S` socket before execution. Both HTTP and tmux run on the bounded background Jobs worker; its output queue applies cancellable backpressure rather than losing completed actions. Weak main-thread callbacks check window generation. Context menus and asynchronous sheets carry the captured tab instance; closing or reopening a key cannot replay an old dialog or worker result.

Authentication precedes hub creation and restoration. User opens defer until restoration finishes, preserving the approved plan's stronger ordering. Restore aliases remain shared with IPC cancellation while background requests run. Background opening keeps the current selection and focus. Closing selects the last remaining tab, preserves running agents, and kills an idle scratch session only after unchanged tmux server/session identity is proved again.

The native file adapter uses the existing Store DocHandle for read-only loading and authoritative writes across Legacy, Mirror, Unified and Sealed. It does not activate a mode or implement SQL. Saves compare the exact previously read bytes under the producer's document lock; external changes fail visibly. Unrestored labels stay in snapshots until restoration completes. History retains at most 80 entries and removes the session duplicate. IPC seeds the exact original three filenames before its owned 500ms timer and never deletes producer-owned files; descriptor checks reject FIFOs/symlinks and oversized or replaced input.

The immutable reference is /home/someguy/codebase/0xJesus/ComandOS/bin/cc-app-mac. The complete 84-node AST ledger is /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m4/docs/verification/mac-app-method-coverage.json. M4 boundaries and verification limits are documented in /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-mac-m4/docs/verification/mac-app-m4-boundaries.md. Linux behavior tests and both Apple type checks do not establish native link, AppKit/WebKit GUI parity, RSS or one-hour acceptance; those gates remain with Root on the Mac.
