# Session Recovery and Crash-Safe Restore Design

**Date:** 2026-07-14

**Status:** Approved for planning

**Incident:** `systemd-oomd` killed the tmux server scope at 09:29:58, terminating every ComandOS session in that process tree. The desktop app then recreated named tabs as empty shells because `app-tabs.json` retained labels but not recovery metadata.

## Goal

Recover all eleven mapped local agent sessions from their on-machine Claude and Codex histories without interrupting the active ComandOS session. Make future desktop restoration remember enough local metadata to resume each agent in the correct project, while reducing the chance that memory pressure destroys every session together.

The saved Heritage SSH tab is reconnected separately through the existing local SSH configuration. No credentials, prompts, histories, or SSH configuration leave the computer.

## Recovery Inventory

| Session | Label | Working directory | Agent |
| --- | --- | --- | --- |
| `term-3692-1` | Signara QUANT | `/home/someguy/codebase/0xJesus/Signara/signara-api-v1` | Claude |
| `term-4045-1` | Securitize | `/home/someguy/codebase/0xJesus/securiritize` | Codex |
| `term-4730-1` | Impulso | `/home/someguy/codebase/0xJesus/Impulso` | Codex |
| `term-4730-4` | Relotto | `/home/someguy/codebase/0xJesus/Relotto` | Claude |
| `term-4730-5` | Blockpay | `/home/someguy/codebase/Blockpay-Network/api-blockapp-2.5` | Claude |
| `term-5038-6` | SAVA | `/home/someguy/codebase/0xJesus/SAVA/SAVA-WebApps` | Claude |
| `term-5694-1` | someguy | `/home/someguy` | Claude |
| `term-6629-4` | Merauto | `/home/someguy/codebase/0xJesus/Merauto/merauto-pos` | Claude |
| `term-8251-2` | NearSDF | `/home/someguy/codebase/0xJesus/NearFlutter` | Codex |
| `term-8751-1` | Scarlett | `/home/someguy/codebase/0xJesus/ScarlettPlattform` | Codex |
| `term-9231-1` | LifeOS | `/home/someguy/codebase/0xJesus/LifeOS` | Codex |

`term-6629-3` is the currently active ComandOS/Codex session and must not be restarted, renamed, or receive injected input.

## Chosen Approach

Recovery is rolling rather than parallel:

1. Snapshot the current tmux layout, tab files, state files, memory, and pressure data.
2. Mark the current application scope as an `avoid` candidate for `systemd-oomd` when supported. This is a temporary incident safeguard, not a substitute for resource isolation.
3. Before touching a target, require that its pane is still an idle shell and its expected project directory exists.
4. Change that pane to the recorded directory and resume the recorded agent from local history:
   - Claude: `claude --continue`
   - Codex: `codex -C <directory> resume --last`
5. Wait for a live agent descendant and verify its working directory before moving to the next target.
6. Stop the rollout if available memory becomes unsafe, memory pressure rises materially, the target is no longer an idle shell, or the agent cannot start. A failure in one tab does not alter the remaining tabs.
7. Reconnect the saved Heritage SSH tab using the existing host alias after local sessions are stable.

The current tab remains active throughout. Existing on-disk histories are backed up, never rewritten as part of operational recovery, and no blanket tmux restart is allowed.

## Persistent Restore Metadata

Keep `app-tabs.json` backward-compatible as the ordered `session -> label` map used by the desktop and remote UI. Add an atomic local metadata file, `~/.claude/hooks/app-tabs-meta.json`, keyed by session:

```json
{
  "term-5038-6": {
    "cwd": "/home/someguy/codebase/0xJesus/SAVA/SAVA-WebApps",
    "agent": "claude",
    "kind": "local"
  },
  "sshtab-heritage-1": {
    "kind": "ssh",
    "host": "heritage"
  }
}
```

Metadata is refreshed atomically whenever tabs are saved and immediately before an intentional close. Values come from the live pane first, then dashboard state, then valid history. Only absolute, existing working directories and known agents are accepted. SSH entries store only the local host alias, never passwords or private keys.

On startup, a missing tmux session is restored from metadata. Legacy installations with labels but no metadata remain supported: ComandOS may open a plain shell, but it must not claim that an agent conversation was resumed. Corrupt or invalid metadata is ignored per tab without preventing other tabs from opening.

## Process and Memory Isolation

The tmux server must no longer be born accidentally inside the first transient `systemd-run --scope` client. When no server exists, ComandOS starts it through a stable user unit dedicated to tmux, and later tmux client commands run directly.

Agent commands launched inside panes run in separate transient user scopes where available. This keeps the lightweight tmux server and idle shells outside an individual agent's resource group and gives `systemd-oomd` a smaller unit to sacrifice under extreme pressure. The dedicated tmux unit is marked `ManagedOOMPreference=avoid`, not `omit`, so the machine can still recover from genuine exhaustion.

Non-systemd platforms continue to launch tmux and agents directly. Failure to create a transient agent scope falls back to direct execution and is surfaced in diagnostics rather than losing the tab.

## Error Handling

- Never send input to a pane unless its current command is a supported idle shell.
- Never replace or kill the active ComandOS session.
- Validate session names, host aliases, agent names, and absolute paths before constructing commands.
- Quote all paths as arguments; do not concatenate saved metadata into shell source.
- Write metadata through a temporary file plus atomic rename.
- Record per-tab recovery outcomes so partial recovery is explicit and retryable.
- Preserve the old labels-only format and tolerate absent metadata.
- A failed SSH reconnect leaves an interactive tab with the error visible; it does not block local recovery.

## Testing and Verification

Implementation follows red-green-refactor with regression tests proving:

1. A missing `term-*` session with valid metadata resumes the correct agent in the correct directory instead of opening HOME.
2. Legacy labels-only data remains readable and falls back safely.
3. Invalid or corrupt metadata cannot inject commands and does not abort restoration of other tabs.
4. Metadata writes preserve tab order/labels and are atomic.
5. The first tmux server is created by the stable unit; later session creation does not wrap a tmux client in a misleading per-session scope.
6. Claude and Codex resume commands are built as argument arrays with the expected cwd.
7. SSH metadata contains only a host alias and uses the existing reconnect path.
8. Existing desktop/remote tab-parity tests remain green.

Operational completion requires fresh evidence that all eleven target sessions have a live expected agent descendant with the expected cwd, the ComandOS session is unchanged, Heritage was reconnected or reports a visible authentication/network error, memory remains healthy, and no target was silently replaced with a shell.

## Out of Scope

- Reconstructing unsaved terminal output after the kernel killed the process tree.
- Uploading or synchronizing histories, credentials, or SSH data.
- Automatically submitting prompts after an agent resumes.
- Disabling `systemd-oomd` globally.
- Repairing the unrelated broken `chrome-guardian.service` restart loop in this recovery change.
