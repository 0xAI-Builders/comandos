# Native project session command

`comandos x` and the `ccx` argv0 alias replace the maintained project-session
Bash command. Directory discovery and the configuration-key parser run in Rust.
Only the injected tmux program creates, kills or attaches sessions. A caller can
provide a program and argument prefix, including an explicit private socket.
Production preserves tmux's environment, interactive attachment and process
replacement. This implementation does not install an alias or run a session as
part of migration verification.

The current original command is the differential oracle. Private HOME fixtures
and a Rust tmux double verify stdout, command argument boundaries and exit codes
for list/empty list, exact kill/failure, direct and logical symlink paths,
two-level case-insensitive exact/fuzzy search, excluded node_modules descendants,
ambiguous/absent/deeper projects, Unicode and wildcard/bracket names, session
reuse, attach/switch, failed new-session/window and final attachment failures.
Harness checks cover flag/environment/config precedence, last definitions,
quoted values, custom launch keys and each builtin. The double only records
arguments; it never starts tmux, a PTY, a GUI or an agent. A separate injected
boundary case ensures kill performs exactly one targeted operation.

Intentional repair: an empty search in the original appends two zero lines from
`grep -c ... || echo 0`, producing an incidental integer-expression warning on
stderr. Native search retains the deliberate `No encontre` message and exit 1
and removes that warning. Differential checks allow only that exact known
warning for an empty search; other stderr must match. Runtime-missing-program
diagnostics name tmux and the native IO error rather than a retired script line.

Patterns compile once for each of the exact and fuzzy searches. Their native
Unicode matching covers standard globs, escaped wildcards, ranges and POSIX
character classes exercised by the oracle; locale-specific collating/equivalence
classes are not an audited gate. Project paths that are not UTF-8 are not an
accepted CLI boundary. No configuration values are exported into the environment.

The other remaining C2 commands, installer publication and a live cutover are
separate outstanding migration work.
