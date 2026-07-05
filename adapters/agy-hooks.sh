#!/usr/bin/env bash
# Antigravity CLI (agy) -> ComandOS. Registro global (cc-agents setup) en
# ~/.gemini/config/hooks.json con formato PLANO (sin wrapper {hooks:[]}):
#   "PreInvocation": [{"type":"command","command":".../agy-hooks.sh working"}]
#   "Stop":          [{"type":"command","command":".../agy-hooks.sh done"}]
# El payload de agy es camelCase, NO trae el nombre del evento (va como $1)
# y el directorio real es workspacePaths[0]. Salida: JSON vacio = no intervenir.
in=$(cat)
e="${1:-done}"
cwd=$(jq -r '.workspacePaths[0] // ""' <<<"$in" 2>/dev/null)
if [ -n "$cwd" ]; then
  "$HOME/.claude/hooks/cc-notify.sh" --agent agy --event "$e" --cwd "$cwd" >/dev/null 2>&1 &
fi
echo '{}'
exit 0
