# Shared desktop policies (M2)

`comandos-desktop` contains no GTK, AppKit, PyObjC or application owner. Its source
forbids unsafe code and inherits the workspace lint. GTK consumes the same types
through reexports and type aliases; it does not translate them into parallel
models. The extraction base is `ca9363a29aa76d3ce4c962efb1aaf81568deb233`.

## Boundaries

| Shared policy | GTK-owned boundary retained |
| --- | --- |
| `tabs::TabRegistry`, its Session/Local/Web kinds and favorite generation | GTK tab widgets and callbacks |
| `restore::RestorePlan`, actions, coordinator and result | RestoreExecutor, tmux/resume/layout execution and cancellation |
| `state_files::StateFiles<C,G>` and `StateError<E>` | `AppConfig` implements `StateConfig`; `WriteGuard` implements `StateGuard` |
| IPC request identity, parser and guarded consumer | GLib file monitor and delivery to the window |
| Ordered GTK bridge, command error, strict string arguments and validation | Bridge installation and GTK command registry/consumers |
| Loopback HTTP, bounded response parsing and isolated socket cancellation | App polling jobs and owner teardown |
| Language decision and default theme | GTK conf-file provider, CSS, widgets and preferences delivery |

The file policy uses the existing Store `DocHandle.read_readonly`, Legacy
authority check, sidecar locks, CAS and guarded publication. It delegates snapshot
persistence to the existing Store helper. It neither creates migration control
files nor broadens the file whitelist. A Mac filesystem backend must implement
these guard ports before it can write. Loading Mac metadata from supplied bytes
does not authorize an `app-tabs-meta.json` writer.

## Mac data contracts

The root `TabKind` and the types in `mac_tabs` preserve the six Mac restoration
kinds. They are separate from GTK registry kinds. Metadata accepts the five
original metadata kinds; Xterm is inferred during restoration. Metadata takes
precedence over project lookup, then session prefixes.

Several plan signatures were illustrative. The original accepts more JSON
values than those signatures express, so the exported contracts preserve them:

- `SavedTabs` stores `(String, Value)`: old dictionary labels can be null, boolean
  or objects. Label merging still uses only nonempty string labels.
- Mac `BridgeMessage` preserves raw theme, session and window `Value`s. A missing
  window defaults to `claude`; explicit null or false remains explicit. GTK's
  strict bridge is a separate parser and retains its original validation.
- `load_tab_metadata` returns `Result<_, MetadataError>`. An array or object kind
  raises the original Python `TypeError`; this is represented as a typed error,
  rather than silently accepting or dropping the document. Invalid JSON and
  non-object documents still produce an empty map.

The Mac ports come from the actual top-level functions and two methods in
`/home/someguy/codebase/0xJesus/ComandOS/bin/cc-app-mac`. Tests execute extracted AST
bodies with inert Foundation, file, HTTP, tmux and clock boundaries. PyObjC and
application startup are never imported. The SSH digit table is the builtin
`str.isdigit` property from the calibrated CPython 3.10.12 / Unicode 13.0.0
oracle, generated over all Unicode code points; it is not a fixture vocabulary.
A different Python Unicode database remains a runtime parity consideration.

`TermUrl` uses an HTTP origin and Python query order/encoding; the page derives
WebSocket origin, so the original file-URL `ws` parameter is omitted as specified
by M2. `RetrySchedule` only computes the original dashboard retry deadline of two
seconds. M3 owns timer registration, window identity, cancellation and teardown.
`history_item` takes explicit observed fields and time; `archive_into` consumes
that item's JSON. No function discovers user processes or signals them.

## Verification and pending runtime gates

`cargo xtask mac-check` executes two offline checks, one per Apple target,
covering both `comandos-app-mac` and `comandos-cli`, and stops on the first failed
check. The CLI entry and command execution are tested using an owned temporary
Cargo fixture. The real command currently refuses the absent AppKit package;
M3 must create that consumer before the Apple build gate can pass.

M2's GTK tests cover existing native consumers, AST bridge/key/theme contracts,
private guarded files, live Legacy CRUD, all four authority modes, stale writes,
MOVED files, IPC replacement, restoration and socket cancellation. They do not
initialize GTK. Mac AppKit execution, GUI equivalence, fonts, CPU/RSS and the
one-hour runtime gate remain outside this headless checkpoint.
