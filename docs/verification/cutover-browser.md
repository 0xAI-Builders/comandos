# Browser broker: prepared smoke and pending production cutover

Code gate B6 is implemented. Production cutover, RSS acceptance, drain and the
24-hour observation gate remain **pending**. Local test fixtures start no browser.

## Verified locations and smoke interface

The controller checkout is
`/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-integration-acp`.
The existing broker on machine **macmini** reads
`/home/john/.config/comandos-browser/config.json`, listens on 19441 and reports to
`/home/john/.local/share/comandos-browser/state/status.json`.
Its unit is `/home/john/.config/systemd/user/comandos-browser.service` on macmini.
These locations were read without modification on 2026-10-06 UTC.

From the controller checkout:

```sh
cargo xtask browser-e2e --host macmini --port 19442 --catalog-only \
  --status-path /home/john/.local/share/comandos-browser/state-rs/status.json
cargo xtask browser-e2e --host macmini --port 19442
```

The first command opens eight MCP clients and verifies 29 tools per client.
It checks a fresh status containing this run's eight unique client names and
requires `workers == 0`; an old status or a status from another broker fails.
The second uses disposable `data:` pages, verifies separate inventories and
snapshots, a PNG image, `ERR_BUSY` for a third worker and capacity release after
disconnect. Each client closes on failure too. Requests have deadlines; replies
and status reads have a 16 MiB limit. SSH uses batch mode and a connection timeout.

`--host ''` uses loopback TCP and requires an absolute private `--status-path`.
The executable integration test starts the Rust broker and a stateful native MCP
double inside a private directory. It also verifies SSH argument ordering through
a native SSH double and rejects deliberately leaked pages and invalid PNG data.
It does not verify real Chromium behavior, production RSS or the 24-hour gate.

## Production procedure, not yet executed

The approved phase 5 plan requires Jesus's OK for each step that writes on
macmini. Prepare exact binaries, hashes, configuration and unit contents for
review before requesting the corresponding production action. No command below
authorizes changing an existing service during private validation.

1. Build and verify the intended release in the controller checkout. Review the
   complete release manifest and consumer protocol gate. Stage the broker on
   macmini under `/home/john/.local/share/comandos-browser/releases/` in a directory
   named by its measured hash, verify its full SHA-256, then atomically replace
   `/home/john/.local/share/comandos-browser/bin/comandos-broker-mac` with the
   reviewed release link. Do not use an unverified hash or a temporary binary.

2. Derive `/home/john/.config/comandos-browser/config-rs.json` on macmini from the
   existing config, preserving command, environment and catalog. Capture the
   **old** `state_dir` before assigning the new one. Set `port = 19442`,
   `state_dir = /home/john/.local/share/comandos-browser/state-rs` and
   `python_status = /home/john/.local/share/comandos-browser/state/status.json`.
   The last path must point to the original Python broker, so both brokers share
   the two-worker limit. Set private configuration permissions to 0600 and the
   new state directory to 0700. Review and install
   `/home/john/.config/systemd/user/comandos-browser-rs.service` on macmini;
   reload units and start this candidate without enabling it yet.

3. Observe both status files before the smoke. Catalog acceptance needs a quiet
   combined worker pool; the normal smoke needs two free slots. Do not disconnect
   a user's browser connection to make room. Run the two commands above when
   capacity is naturally available. Measure each service's `MemoryCurrent` at
   rest and with two workers, retaining timestamps and the measured executable
   hash in
   `/home/someguy/codebase/0xJesus/ComandOS/.worktrees/rust-integration-acp/docs/verification/rss.jsonl`.
   The Rust idle value must be no greater than the Python idle measurement.

4. After the release and installer gates pass, stage the reviewed local release
   and replace the three client aliases under `/home/someguy/.local/bin/`:
   `/home/someguy/.local/bin/cc-browser-remote`,
   `/home/someguy/.local/bin/cc-browser-npx-guard` and
   `/home/someguy/.local/bin/cc-browser-expose`.
   First verify a disposable session through the Rust client while it still
   connects to the Python broker on 19441. Any test tmux uses its own socket.

5. Review the exact local 0600 config
   `/home/someguy/.config/comandos/browser.json` containing host `macmini` and
   port 19442. After approval, write it atomically and verify a new disposable
   session plus a fresh candidate status showing its connection. Existing
   connections retain their original broker.

6. Read the original status on macmini until `connections == 0` in two samples
   separated by ten minutes. Record both actual timestamps and values. Only then
   enable the Rust unit and disable/stop the Python unit with the approved action.
   Keep the original binary, config and unit for rollback through phase 6.

7. Before step 6, rollback removes the local browser config to route new sessions
   back to 19441; use the reviewed installer rollback if client aliases need
   restoration. After step 6, first enable/start the original Python service on
   macmini, then remove the local browser config. Stop the Rust service only
   after its own connection count reaches zero. Preserve both sets of evidence.

8. Acceptance requires a passing real smoke against 19442, the measured idle RSS
   bound, no migration-caused errors or forced closures in live browser sessions,
   and candidate status freshness below ten seconds across an actual continuous
   24-hour observation interval. Review both service journals for that interval.
   A private smoke or a backdated report cannot satisfy this gate.
