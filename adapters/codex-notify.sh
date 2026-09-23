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

# MISMA clave de estado que hooks/cc-notify.sh (pane-aware bajo tmux): si no
# coincide, el dedupe nunca encuentra el "done" reciente y Codex avisa 2 veces.
codex_state_file() { # $1 = cwd
  local proj proj_file sess key
  proj=$(basename "$1")
  proj_file=$(printf '%s' "$proj" | tr -c 'A-Za-z0-9._-' '-' | head -c 80)
  key="$proj_file"
  if printf '%s' "${TMUX_PANE:-}" | grep -Eq '^%[0-9]+$' && command -v tmux >/dev/null 2>&1; then
    sess=$(tmux display-message -p -t "$TMUX_PANE" '#S' 2>/dev/null || true)
    printf '%s' "$sess" | grep -Eq '^[A-Za-z0-9._-]{1,80}$' \
      || sess=$(printf '%s' "$proj" | tr '.:' '--' | head -c 60)
    [ -n "$sess" ] && key="${proj_file}--${sess}--${TMUX_PANE#%}"
  fi
  printf '%s/.claude/hooks/state/%s.json' "$HOME" "$key"
}
state=$(codex_state_file "$cwd")
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
