#!/usr/bin/env bash
# Codex -> ComandOS. Registro (lo hace `cc-agents setup`):
#   ~/.codex/config.toml:  notify = ["/ruta/a/adapters/codex-notify.sh"]
# Codex invoca este script con UN argumento JSON al terminar cada turno.
# Es fallback legacy: los lifecycle hooks ricos viven en codex-hooks.sh.
payload="${1:-}"
[ -z "$payload" ] && exit 0
type=$(printf '%s' "$payload" | jq -r '."type" // ""' 2>/dev/null)
[ "$type" = "agent-turn-complete" ] || exit 0
cwd=$(printf '%s' "$payload" | jq -r '.cwd // ."workspace-path" // empty' 2>/dev/null)
[ -n "$cwd" ] || cwd="$PWD"
last=$(printf '%s' "$payload" | jq -r '."last-assistant-message" // ""' 2>/dev/null)

proj=$(basename "$cwd")
proj_file=$(printf '%s' "$proj" | tr -c 'A-Za-z0-9._-' '-' | head -c 80)
state="$HOME/.claude/hooks/state/$proj_file.json"
if [ -f "$state" ]; then
  now=$(date +%s)
  if jq -e --argjson now "$now" '
    (.agent // "") == "codex"
    and (.status // "") == "done"
    and (($now - ((.ts // 0) | tonumber)) <= 15)
  ' "$state" >/dev/null 2>&1; then
    exit 0
  fi
fi

exec "$HOME/.claude/hooks/cc-notify.sh" --agent codex --event done --cwd "$cwd" --full "$last"
