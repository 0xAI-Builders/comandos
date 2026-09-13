#!/usr/bin/env bash
# Tool telemetry for ComandOS: forwards names/timing/status only, never payloads.
set -u
input=$(cat)
event=$(jq -r '.hook_event_name // ""' <<<"$input" 2>/dev/null)
case "$event" in
  PreToolUse) phase=start ;;
  PostToolUse)
    if jq -e '(.tool_response | type == "object") and ((.tool_response.is_error // false) == true or (.tool_response | has("error")))' <<<"$input" >/dev/null 2>&1; then
      phase=failed
    else
      phase=success
    fi ;;
  *) exit 0 ;;
esac
tool=$(jq -r '.tool_name // ""' <<<"$input" 2>/dev/null)
skill=""
if [ "$tool" = "Skill" ]; then
  skill=$(jq -r 'if (.tool_input.skill | type) == "string" then .tool_input.skill else "" end' <<<"$input" 2>/dev/null)
  printf '%s' "$skill" | grep -Eq '^[A-Za-z0-9_][A-Za-z0-9_.:@/-]{0,159}$' || skill=""
fi
tool_id=$(jq -r '.tool_use_id // ""' <<<"$input" 2>/dev/null)
[ -n "$tool" ] || exit 0
pane="${TMUX_PANE:-}"
printf '%s' "$pane" | grep -Eq '^%[0-9]+$' || exit 0
session=$(tmux display-message -p -t "$pane" '#S' 2>/dev/null || true)
printf '%s' "$session" | grep -Eq '^[A-Za-z0-9._-]{1,80}$' || exit 0
script="$HOME/.local/bin/cc_usage.py"
[ -r "$script" ] || exit 0
jq -cn --arg phase "$phase" --arg session "$session" --arg pane "$pane" \
  --arg tool "$tool" --arg skill "$skill" --arg tool_id "$tool_id" --argjson at "$(($(date +%s%N) / 1000000))" \
  '{phase:$phase,tmux_session:$session,tmux_pane:$pane,tool_name:$tool,skill_name:$skill,tool_use_id:$tool_id,at_ms:$at,confidence:"exact"}' \
  | python3 "$script" tool-event >/dev/null 2>&1 &
exit 0
