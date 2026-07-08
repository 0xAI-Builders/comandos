#!/usr/bin/env bash
# Gemini CLI / Antigravity CLI -> ComandOS. Registro (lo hace `cc-agents setup`):
# hooks BeforeAgent/AfterAgent/Notification/SessionEnd en settings.json.
# Recibe el JSON del hook por stdin. Antigravity: exporta CC_AGENT=agy.
in=$(cat)
agent="${CC_AGENT:-gemini}"
ev=$(jq -r '.hook_event_name // ""' <<<"$in" 2>/dev/null)
cwd=$(jq -r '.cwd // ""' <<<"$in" 2>/dev/null)
msg=$(jq -r '.message // ""' <<<"$in" 2>/dev/null)
case "$ev" in
  BeforeAgent)  e=working; full="" ;;
  AfterAgent)   e=done;    full=$(jq -r '.prompt_response // ""' <<<"$in" 2>/dev/null) ;;
  Notification) e=waiting; full="$msg" ;;
  SessionEnd)   e=end;     full="" ;;
  *) exit 0 ;;
esac
# En segundo plano y callados: los hooks de gemini son sincronos y no debe
# imprimirse NADA a stdout.
"$HOME/.claude/hooks/cc-notify.sh" --agent "$agent" --event "$e" --cwd "$cwd" \
  --msg "$msg" --full "$full" >/dev/null 2>&1 &
exit 0
