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
matcher uses character tokens and dynamic programming for `*`/`?`, literals,
escapes, bracket sets and ranges. Literals and range endpoints use simple
character lowercase, avoiding Unicode full-fold equivalences such as long s
and ASCII s. Like the observed GNU fnmatch, it also tries a byte matcher after
the character matcher fails: both `?` and `??` can match two-byte É. The byte
matcher uses ASCII case conversion and byte classes. GNU C.UTF-8 POSIX classes
in the character matcher test the original character: upper/lower
remain distinct despite case-insensitive matching; digit/xdigit are ASCII;
alpha/alnum include Unicode letters and non-ASCII decimal digits. C.UTF-8 space,
blank, print, graph, control and punctuation distinctions are also tested,
including NBSP, combining marks and invisible controls. The existing regex
dependency remains only for decimal/assigned Unicode category predicates,
compiled once without any case-insensitive option; it no longer matches globs.

Discovery checks the root's non-following metadata, preserving GNU find -P for
both a symlinked root and descendant directory symlinks. Explicit requested
symlink directories remain accepted with their logical path. Filesystem DFS
order, tmux argv, final exec and configuration handling are unchanged.

The audit locale remains C.UTF-8. Complete Unicode-version equivalence across
Rust/glibc versions and locale-specific collating/equivalence classes are not
proven by these fixtures. Project paths that are not UTF-8 are not an accepted
CLI boundary. No configuration values are exported into the environment.

The other remaining C2 commands, installer publication and a live cutover are
separate outstanding migration work.
