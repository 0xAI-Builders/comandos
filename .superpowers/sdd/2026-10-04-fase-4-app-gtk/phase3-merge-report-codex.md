# Fixed Phase 3 prerequisite merge for GTK

Merged the approved fixed commit b9eea80 into migration/rust-fase4 from a4c3797. No source changes to the terminal engine; preserves shared history instead of copying crates. No deployment, GUI, Xvfb, personal sessions or services.

Resolved five conflicts by preserving both intents:

- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/Cargo.toml: union of GTK and Phase 3 workspace members, retaining the vendored alacritty patch and Phase 3 feature profiles.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/Cargo.lock: union of pinned package identities and dependency lists, then Cargo offline validated/pruned the resolution.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-core/src/lib.rs: retain usage_state and web_assets along with automatically merged malloc_tuning.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/crates/comandos-server/Cargo.toml: retain usage reqwest and add Phase 3 futures sink and Tokio fs features.
- /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/src/main.rs: retain app-drift and all Phase 3 web subcommands and combined usage help.

The automatic manifest merge also duplicated the identical sha2 dependency in /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-fase4/xtask/Cargo.toml; removed the duplicate.

Offline cargo check for comandos-app, comandos-server and xtask with -j2 passes. Explicit native app suites (lib, config, proc_jobs, tmux_guard, write_guard) validate the merged prerequisite without running gtk_smoke. GTK T4 test remains untracked and outside this merge; its RED failure (missing term adapter) was captured before implementation. No broad Phase 3 revalidation required by controller; visual/browser gates remain remote-only.

Final native result: 53/53 app tests pass (lib1, config19, proc_jobs6, tmux_guard20, write_guard7). No GTK initialization or display server was invoked. Formatting and diff checks pass; all merge markers resolved. The T4 test is intentionally excluded from the prerequisite merge and still fails until the adapter is implemented.
