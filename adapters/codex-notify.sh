#!/usr/bin/env bash
# Codex -> ComandOS. Registro (lo hace `cc-agents setup`):
#   ~/.codex/config.toml:  notify = ["/ruta/a/adapters/codex-notify.sh"]
# Codex invoca este script con UN argumento JSON al terminar cada turno.
payload="${1:-}"
[ -z "$payload" ] && exit 0
type=$(printf '%s' "$payload" | jq -r '."type" // ""' 2>/dev/null)
[ "$type" = "agent-turn-complete" ] || exit 0
cwd=$(printf '%s' "$payload" | jq -r '.cwd // ."workspace-path" // empty' 2>/dev/null)
[ -n "$cwd" ] || cwd="$PWD"
last=$(printf '%s' "$payload" | jq -r '."last-assistant-message" // ""' 2>/dev/null)
exec "$HOME/.claude/hooks/cc-notify.sh" --agent codex --event done --cwd "$cwd" --full "$last"
