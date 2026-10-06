# Native desktop shortcuts

`comandos raise app` and `comandos raise term` accept the `cc-centro` and
`cc-term` invocation aliases. They retain the original window lookup order,
stdout/stderr handling, ignored trailing arguments and process exit status.
App searches `comandos`; term searches `kitty`, `comandos`, then `tilix`.
Window tool errors remain silent and allow the original fallback.

The replacement fallback calls the installed Rust desktop executable through
`setsid -f`, and the installed CLI with the `next` subcommand through process
replacement. Both executables are resolved under the current HOME's private
installation directory. The former replaces the Python app; the latter
replaces `cc-next`. Native `next`, app release packaging and live
shortcut installation remain separate migration tasks. No user shortcuts,
windows or processes were modified by this checkpoint. A missing native next
executable returns 127 with a native diagnostic; another exec failure returns
126. The original shell diagnostic is not retained for this replacement.

Three differential test groups execute the original Bash sources and the
actual CLI binary with only Rust fake tools on PATH. They cover every window
priority, both aliases, both native subcommands, inherited next output,
suppressed launcher output, fallback nonzero status, missing window/launcher
tools, non-executable launcher status 126 and ignored arguments. No real
`wmctrl`, `setsid`, desktop app, next command, browser or tmux runs.

Validation source:
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-integration-acp/crates/comandos-cli/tests/raise.rs

Original sources:
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-integration-acp/bin/cc-centro
and
/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-integration-acp/bin/cc-term.

Red, permission regression and green evidence:
/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/cli-raise-red.log,
/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/cli-raise-permission-red.log,
/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/cli-raise-green.log
and
/home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/cli-raise-clippy.log.
