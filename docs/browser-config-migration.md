# Browser MCP configuration migration

`bin/cc-browser-remote` streams MCP messages through SSH to the Mac mini broker at `127.0.0.1:19441`. The SSH host alias is `macmini`. It uses direct TCP forwarding with `ssh -W`, so connections do not require an SSH session channel. Authentication and forwarding failures terminate the client.

The migration tool requires Python 3.11 or newer. Its default client path is `/home/someguy/.local/bin/cc-browser-remote`. Install and validate that client and the remote broker before applying a configuration plan. Installing the client and operating the broker are separate from this tool.

## Preview and apply

Run these commands from the repository root. Discovery only lists existing candidate paths:

```sh
python3.11 lib/browser_config_migration.py discover
```

Discovery checks `~/.claude.json`, provider directories such as `~/.claude`, `~/.codex` and `~/.grok`, sibling directories with provider prefixes, and immediate account/profile directories beneath those roots. It also checks roots from `CLAUDE_CONFIG_DIR`, `CODEX_HOME` and `GROK_HOME` in the current environment and readable processes owned by the current user. Other environment values are discarded. Use `discover --no-process-env` to omit process environments or `--home PATH` to change the home-directory search base.

Choose the files explicitly when making the plan:

```sh
python3.11 lib/browser_config_migration.py dry-run \
  --config /home/someguy/.claude.json \
  --config /home/someguy/.claude/settings.json \
  --config /home/someguy/.codex/config.toml \
  --config /home/someguy/.grok/config.toml \
  --plan /tmp/browser-migration-plan.json
```

Add repeated `--config PATH` arguments for discovered account configurations. Only include files that exist. `--wrapper PATH` selects a different absolute client path. The plan stores paths, content hashes and file identities, with mode `0600`; it contains no configuration values. The command reports which files would change.

Apply the reviewed plan:

```sh
python3.11 lib/browser_config_migration.py apply \
  --plan /tmp/browser-migration-plan.json \
  --backup-dir /home/someguy/.local/state/browser-migration-backups
```

Apply verifies every reviewed file's SHA-256 hash and file identity before creating backups or changing configurations. Each original receives a `0600` backup in a unique `0700` directory, with a manifest mapping backups to original paths. Configuration replacements are atomic per file and retain the original mode. A second preview of migrated files reports no changes.

## Changes and limits

The exact server names `chrome-bg`, `chrome-current`, `chrome-devtools` and `chrome-devtools-current` become a single `chrome-bg` entry in each existing server map that contains one of those names. JSON migration handles the global `mcpServers` map and project `mcpServers` maps. It also sets the exact plugin key `chrome-devtools-mcp@chrome-devtools-plugins` to `false` when present. Personal `claude-in-chrome` entries and unrelated plugin settings remain intact.

TOML migration replaces targeted `mcp_servers` table blocks, including nested environment tables, while retaining unrelated text and comments. It checks the parsed result against the intended configuration. Unsupported inline layouts are rejected. JSON migration preserves unrelated values but reformats changed documents with two-space indentation; duplicate keys are rejected. Symlink configuration files are rejected.

Pause configuration-writing applications while applying. Hashes are checked again after backup creation and immediately before each replacement, but unrelated writers do not share a lock with this tool. Several files cannot be replaced in one filesystem transaction. A failure during replacement may leave earlier files migrated; retain the reported backups and inspect their manifest before restoring. Restoring a backup overwrites subsequent edits to that file.

Existing MCP client processes can retain their loaded configuration. Start a fresh client connection after migration to verify the new server.

## Cached npx commands

`bin/cc-browser-npx-guard` handles clients that still cache an `npx chrome-devtools-mcp` launch. It recognizes only `chrome-devtools-mcp` or `chrome-devtools-mcp@VERSION` as the first package, optionally preceded by `-y`, `--yes` or `--no-install`. These launches execute `$HOME/.local/bin/cc-browser-remote` directly and discard old browser arguments. Remote-client failure is returned to the caller.

Every other invocation, including `--version`, a leading `--`, and another package followed by a Chrome DevTools argument, delegates all original arguments to the original Node executable and npm CLI. The guard is pinned to `/home/someguy/.nvm/versions/node/v22.19.0/bin/node` and `/home/someguy/.nvm/versions/node/v22.19.0/lib/node_modules/npm/bin/npx-cli.js`.

Install only after verifying that `/home/someguy/.nvm/versions/node/v22.19.0/bin/npx` is still a symlink to `../lib/node_modules/npm/bin/npx-cli.js`, and that the remote client works. Save the symlink itself in a private backup directory. Stage an executable copy of the guard beside `bin/npx`, then atomically replace the symlink with that staged file. Do not copy through the symlink, which would overwrite npm's JavaScript entry point. Restore the saved symlink to remove the guard. A Node version change requires checking the new `npx` path before installing another guard.

## Local development ports

The Mac browser needs a reverse SSH forward to reach a development server running on this laptop. Start one with the repository command:

```sh
bin/cc-browser-expose start 3000
bin/cc-browser-expose status
bin/cc-browser-expose stop 3000
```

The first command requests a listener on the Mac's `127.0.0.1:3000` that forwards to the laptop's `127.0.0.1:3000`. Open `http://127.0.0.1:3000` in the remote browser. An optional second port changes the Mac listener, for example `bin/cc-browser-expose start 80 18080`. Remote ports must be between 1024 and 65535. Start the local development server separately.

Each mapping owns a dedicated SSH master and control socket. Repeating the same mapping checks its existing master; a different local port on the same remote port is rejected. `stop` addresses only that mapping's master. `status` checks recorded masters without creating or changing state. SSH must report successful forwarding and pass `-O check` before the command reports a started tunnel. This confirms the tunnel, not the health of the development server.

State lives in `$XDG_RUNTIME_DIR/cc-browser-forwards` when that variable exists, otherwise `~/.local/state/cc-browser-forwards`. `CC_BROWSER_FORWARD_DIR` can select a shorter absolute private directory if an SSH control path exceeds the platform limit. The directory must be owned by the current user with mode `0700`; mapping files use `0600`. The listener requests loopback binding, and the Mac SSH server must honor that address rather than force wildcard listeners through `GatewayPorts yes`.

Implementation: [migration module](../lib/browser_config_migration.py), [SSH client](../bin/cc-browser-remote), and [tests](../tests/test_browser_config_migration.py). Tests use temporary configurations and a fake SSH executable; they do not establish a remote browser session.
