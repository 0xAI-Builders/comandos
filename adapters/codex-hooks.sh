#!/usr/bin/env bash
# Codex lifecycle hooks -> ComandOS.
# Reads Codex hook JSON on stdin and forwards a normalized event to cc-notify.sh.
set -uo pipefail

payload=$(cat)
[ -z "$payload" ] && exit 0

notify="$HOME/.claude/hooks/cc-notify.sh"
[ -x "$notify" ] || exit 0

jq_get() {
  printf '%s' "$payload" | jq -r "$1" 2>/dev/null
}

event=$(jq_get '.hook_event_name // .event // ""')
cwd=$(jq_get '.cwd // ."workspace-path" // empty')
[ -n "$cwd" ] || cwd="$PWD"

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

recent_codex_done() {
  [ -f "$state" ] || return 1
  local now
  now=$(date +%s)
  jq -e --argjson now "$now" '
    (.agent // "") == "codex"
    and (.status // "") == "done"
    and (($now - ((.ts // 0) | tonumber)) <= 15)
  ' "$state" >/dev/null 2>&1
}

case "$event" in
  UserPromptSubmit)
    exec "$notify" --agent codex --event working --cwd "$cwd"
    ;;
  Stop)
    recent_codex_done && exit 0
    full=$(jq_get '.last_assistant_message // ."last-assistant-message" // .message // ""')
    exec "$notify" --agent codex --event done --cwd "$cwd" --full "$full"
    ;;
  PermissionRequest)
    tool=$(jq_get '
      [.tool_name?, .tool?, .tool_call?.name?, .request?.tool_name?, .permission?.tool_name?]
      | map(if type == "object" then (.name // empty) else . end)
      | map(select(type == "string" and length > 0))
      | first // ""
    ')
    command=$(jq_get '
      [.command?, .tool_call?.command?, .input?.command?, .tool_input?.command?,
       .arguments?.command?, .params?.command?]
      | map(select(type == "string" and length > 0))
      | first // ""
    ')
    reason=$(jq_get '
      [.reason?, .description?, .message?, .approval_request?.reason?, .request?.reason?]
      | map(select(type == "string" and length > 0))
      | first // ""
    ')
    msg="Codex necesita permiso"
    [ -n "$tool" ] && msg="$msg: $tool"
    full="$msg"
    [ -n "$command" ] && full="$full
Command: $command"
    [ -n "$reason" ] && full="$full
Reason: $reason"
    exec "$notify" --agent codex --event waiting --cwd "$cwd" --msg "$msg" --full "$full"
    ;;
esac

exit 0
