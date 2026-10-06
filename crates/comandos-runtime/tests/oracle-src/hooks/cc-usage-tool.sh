#!/usr/bin/env bash
# Tool telemetry for ComandOS: forwards names/timing/status only, never payloads.
# Corre en CADA Pre/PostToolUse: una sola jq y un tmux display-message; la
# validacion va con [[ =~ ]] (sin grep) y el envio a cc_usage.py en background
# con stdio a /dev/null para que el harness no espere nada.
set -u
pane="${TMUX_PANE:-}"
[[ "$pane" =~ ^%[0-9]+$ ]] || exit 0
script="$HOME/.local/bin/cc_usage.py"
[ -r "$script" ] || exit 0
session=$(tmux display-message -p -t "$pane" '#S' 2>/dev/null || true)
[[ "$session" =~ ^[A-Za-z0-9._-]{1,80}$ ]] || exit 0
# Milisegundos: EPOCHREALTIME (bash 5) sin fork; si no, date (+%s%N no existe
# en macOS: ahi caemos a segundos*1000).
if [ -n "${EPOCHREALTIME:-}" ]; then
  at="${EPOCHREALTIME/[.,]/}"; at=$(( 10#$at / 1000 ))
else
  at=$(date +%s%N 2>/dev/null)
  case "$at" in ''|*[!0-9]*) at=$(( $(date +%s) * 1000 )) ;; *) at=$(( at / 1000000 )) ;; esac
fi
event=$(jq -c --arg session "$session" --arg pane "$pane" --argjson at "$at" '
  (.hook_event_name // "") as $ev
  | (if $ev == "PreToolUse" then "start"
     elif $ev == "PostToolUse" then
       (if (.tool_response | type) == "object"
           and ((.tool_response.is_error // false) == true or (.tool_response | has("error")))
        then "failed" else "success" end)
     else empty end) as $phase
  | (.tool_name // "" | if type == "string" then . else tojson end) as $tool
  | select($tool != "")
  | (if $tool == "Skill" then (try .tool_input.skill catch "") else "" end) as $raw_skill
  | (if ($raw_skill | type) == "string"
        and ($raw_skill | test("\\A[A-Za-z0-9_][A-Za-z0-9_.:@/-]{0,159}\\z"))
     then $raw_skill else "" end) as $skill
  | {phase:$phase,tmux_session:$session,tmux_pane:$pane,tool_name:$tool,skill_name:$skill,
     tool_use_id:(.tool_use_id // "" | if type == "string" then . else tojson end),
     at_ms:$at,confidence:"exact"}' 2>/dev/null)
[ -n "$event" ] || exit 0
printf '%s\n' "$event" | python3 "$script" tool-event >/dev/null 2>&1 &
exit 0
