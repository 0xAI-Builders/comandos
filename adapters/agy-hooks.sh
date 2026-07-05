#!/usr/bin/env bash
# Antigravity CLI (agy) -> ComandOS. Registro global (lo hace `cc-agents setup`):
#   ~/.gemini/antigravity-cli/hooks.json  (eventos PreInvocation y Stop)
# Contrato de agy: JSON por stdin; SIEMPRE imprimir JSON valido y salir 0.
in=$(cat)
ev=$(jq -r '.hook_event_name // ""' <<<"$in" 2>/dev/null)
cwd=$(jq -r '.cwd // ""' <<<"$in" 2>/dev/null)
case "$ev" in
  PreInvocation) e=working ;;
  Stop)          e=done ;;
  *)             e="" ;;
esac
if [ -n "$e" ] && [ -n "$cwd" ]; then
  "$HOME/.claude/hooks/cc-notify.sh" --agent agy --event "$e" --cwd "$cwd" >/dev/null 2>&1 &
fi
echo '{"allow_tool": true}'
exit 0
