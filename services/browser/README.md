# ComandOS Browser Service

Chrome automation runs on the Mac mini (`macmini`, Ubuntu 26.04), with a Python broker bound to `127.0.0.1:19441`. Laptop clients run only SSH TCP forwarding. There is no local Node, npm or Chrome fallback in the new client.

## Deployment state — 2026-09-11

The migration is active. Six user/account configurations were backed up and deduplicated, the duplicate Chrome DevTools plugin was disabled, and both legacy launcher paths now forward to the Mac. A narrowly scoped npx compatibility guard redirects cached Chrome DevTools commands from already-running clients; unrelated npx commands still use the original npm CLI.

The administrator installed the exact Chrome AppArmor profile. Production tests passed with the sandbox enabled: data-page navigation, external HTTPS, accessibility snapshots, screenshots, isolation between two browser sessions, the two-worker limit and release after disconnect. Sixty unit tests passed. Eight simultaneous catalog clients created no workers. A real SSH reverse tunnel reached a disposable laptop HTTP server; an unavailable broker failed closed without local fallback.

Activation closed 37 idle local MCP servers and their launchers (127 processes, about 5.57 GiB proportional memory at measurement), then closed the unused dedicated local automation browser. Original AI processes, personal Chrome/Brave and tmux pane/layout identities were preserved. The final process check found no local Chrome DevTools MCP server or dedicated chrome-bg browser. The Mac broker used about 15 MB while clients were connected but no browser worker was active. Measurements are observations from this deployment, not memory guarantees.

For a future reinstall, the narrowly scoped AppArmor installer is available at:

```sh
ssh -t macmini 'sudo /home/john/.local/share/comandos-browser/admin/install-chrome-apparmor.sh'
```

The profile and installer are in [apparmor](apparmor/). The installed production wrapper, broker and configuration do not use `--no-sandbox`.

## Runtime and limits

Remote root: `/home/john/.local/share/comandos-browser`.

- Node 22.19.0, Chrome for Testing 153.0.8010.36, Chrome DevTools MCP 1.9.0.
- `runtime/bin/chrome` provides user-scoped shared libraries/fonts and headless mode; no global package installation.
- `~/.config/comandos-browser/config.json` defines the broker command and catalog.
- `~/.config/systemd/user/comandos-browser.service` starts the broker; user linger was already enabled.
- Two isolated browser workers at most; connections and tool listings alone create none.
- At most 16 callers wait for a worker, for up to 20 seconds; otherwise the caller gets a busy result. Each connection allows four outstanding requests.
- Worker leases expire after five minutes without tool calls. Expiry closes pages and explicitly reports the reset before further work.
- Tools have a 120-second deadline. Cancellation does not free an executing slot. Disconnect and timeout terminate the owning worker and verify its detached browser processes have exited before capacity is reused.
- The user service has `MemoryHigh=4G`, `MemoryMax=6G`, `CPUQuota=200%`, and `TasksMax=512`. These bound this service, not total Mac memory consumption.
- MCP messages are limited to 8 MiB. Output backpressure has a deadline.
- Update checks and usage statistics are disabled. The package is pinned.

Sources and runtime checksums are recorded in [runtime-manifest.json](runtime-manifest.json). Keep `TMPDIR=/tmp`; long temporary paths can exceed Chrome's Unix socket path limit.

## Usage

`cc-browser-remote` is the MCP stdio command for Claude, Codex and Grok. It uses the existing SSH alias/key and direct TCP forwarding, so the broker is not exposed on the LAN or Internet.

Each MCP connection owns its browser pages and temporary profile. Personal browser cookies are not copied. Login state expires with that temporary profile; persistent authenticated profiles are a separate feature, not promised by this implementation.

To let the remote browser reach a laptop development server:

```sh
cc-browser-expose start 3000
# The Mac browser can now visit http://127.0.0.1:3000
cc-browser-expose status
cc-browser-expose stop 3000
```

For local nginx on port 80, use `cc-browser-expose start 80 18080`; a name such as `project.localhost:18080` can preserve nginx's Host-based routing. The actual server's allowed-host rules still apply. Upload/download paths refer to the Mac filesystem; transfer files explicitly with SCP when needed.

The [configuration migration guide](../../docs/browser-config-migration.md) explains deduplication, private backups and hash checks. The separate personal `claude-in-chrome` extension is preserved.

## Operations

```sh
ssh macmini 'systemctl --user status comandos-browser.service'
ssh macmini 'cat ~/.local/share/comandos-browser/state/status.json'
ssh macmini 'journalctl --user -u comandos-browser.service -n 40 --no-pager'
```

Status records client names, worker identities, busy state and queue count. It does not record prompts, page contents or credentials. Restarting the service closes its automation browsers and MCP connections; it never starts local replacements.

## Verification

```sh
pytest -q tests/test_browser_broker.py tests/test_browser_config_migration.py tests/test_browser_expose.py
python3 tests/e2e_remote_browser.py --catalog-only
python3 tests/e2e_remote_browser.py
```

The full smoke test creates only disposable data pages. It checks navigation, snapshots, screenshots, isolated page inventories, the two-worker limit and slot release after disconnect.
