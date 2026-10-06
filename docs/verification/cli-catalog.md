# Native CLI catalogue tools

The native entry is `cargo xtask cli-catalog`. With no arguments, or with `build`, it regenerates the catalogue at /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-xtask-catalog/config/cli-commands.json from the five frozen inputs in /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-xtask-catalog/tools/cli-commands/scraped. The checkout root is compiled into the tool, so the result does not depend on the current directory. Original scripts remain as unchanged test references until retirement gates pass.

| Native operation | Original reference |
|---|---|
| `build` | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-xtask-catalog/tools/cli-commands/build.py |
| `builtin-filter INPUT BINARY` | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-xtask-catalog/tools/cli-commands/builtin_filter.py |
| `scrape-prefix SOCKET SESSION REGEX LEAD [MAXV]` | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-xtask-catalog/tools/cli-commands/scrape_prefix.py |
| `verify-names SOCKET SESSION NAMES` | /home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-xtask-catalog/tools/cli-commands/verify_names.py |

The catalogue preserves CLI order, last metadata for duplicate IDs, metadata field order, stable command sorting, duplicate aliases, Grok aliases, argument chips and Python's Unicode JSON indentation. The frozen catalogue has 45,345 bytes and SHA256 `0d9d65a79d30dc4fd24ea94dbfd6ad6775f26726ef5dfc70cf04d27a5f167792`; regeneration is byte-identical. No version, model or menu is fetched from a live agent.

Binary filtering removes the exact trailing built-in marker, strips Python whitespace, takes 40 Unicode scalar values and searches their UTF-8 bytes. Scraping preserves the original depth-four traversal, overwrite rules, retry counts, delays, clears and JSON output. Every tmux invocation includes the explicitly supplied `-L` socket and session; the tool never sends Enter. Child exit statuses are ignored as in the originals; spawn and invalid UTF-8 failures return an error.

Nine private tests compare the original scripts and native implementation, including full catalogue bytes, Unicode whitespace, aliases, duplicate metadata, prefix recursion, source description updates, prompt retries, missing descriptions, sorted names, Unicode13 word membership and universal line boundaries. The unchanged scripts run in isolated Python test processes. Scraping tests replace only subprocess and sleep collaborators and compare every keyboard, capture and delay call. A separate real process double checks the native tmux argv and output decoding. Tests never start or contact a real tmux server or agent.

The supported scraping regex boundary is explicit: Rust regex syntax, with `\w`, `\W`, `\s` and `\S` translated to the original Python 3.10/Unicode13 classes. Python-specific digit/boundary escapes, lookaround and backreferences are rejected before keyboard activity. Full arbitrary Python regex compatibility is not established. Integer MAXV retains the original i64-representable values; malformed inputs produce a concise error rather than a Python traceback. No retirement or live catalogue-scraping acceptance is claimed by this checkpoint.

Native tests, all-target xtask clippy, formatting and diff checks pass. Evidence is recorded at /home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/cli-catalog-verification.json. HOME, all XDG directories and TMP are private directories outside Git; the environment is recorded at /home/someguy/codebase/0xJesus/ComandOS/.scratch/takeover-20261005/cli-catalog-private.json.
