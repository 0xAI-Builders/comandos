#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# shellcheck source=/dev/null
. "$ROOT/lib/platform.sh"

CMD="$HOME/.claude/hooks/cc-notify.sh"
EVENTS="UserPromptSubmit Stop Notification SessionEnd"

count_cmd() { # $1=settings $2=evento — cuántas veces está registrado CMD
  jq --arg cmd "$CMD" --arg ev "$2" \
    '[.hooks[$ev][]? | (.hooks // [])[] | select(.command == $cmd)] | length' "$1"
}

# Case 1: settings.json inexistente -> se crea con los 4 eventos
S="$TMP/fresh/settings.json"
CC_CLAUDE_SETTINGS="$S" cc_register_claude_hooks "$CMD"
for ev in $EVENTS; do
  n=$(count_cmd "$S" "$ev")
  [ "$n" = "1" ] || { echo "fresh: expected 1 hook for $ev, got $n"; exit 1; }
done

# Case 2: idempotente -> segunda corrida no duplica
CC_CLAUDE_SETTINGS="$S" cc_register_claude_hooks "$CMD"
for ev in $EVENTS; do
  n=$(count_cmd "$S" "$ev")
  [ "$n" = "1" ] || { echo "idempotent: expected 1 hook for $ev, got $n"; exit 1; }
done

# Case 3: respeta hooks ajenos y entradas vacías existentes
S="$TMP/existing.json"
cat > "$S" <<'EOF'
{
  "model": "opus",
  "hooks": {
    "Stop": [{"matcher": "*", "hooks": []}],
    "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "/otro/hook.sh"}]}]
  }
}
EOF
CC_CLAUDE_SETTINGS="$S" cc_register_claude_hooks "$CMD"
for ev in $EVENTS; do
  n=$(count_cmd "$S" "$ev")
  [ "$n" = "1" ] || { echo "existing: expected 1 hook for $ev, got $n"; exit 1; }
done
[ "$(jq -r '.model' "$S")" = "opus" ] || { echo "existing: lost unrelated keys"; exit 1; }
[ "$(jq '[.hooks.UserPromptSubmit[] | (.hooks // [])[] | select(.command == "/otro/hook.sh")] | length' "$S")" = "1" ] \
  || { echo "existing: lost foreign hook"; exit 1; }
[ "$(jq '.hooks.Stop | length' "$S")" = "2" ] \
  || { echo "existing: empty Stop entry was not preserved"; exit 1; }

# Case 4: JSON inválido -> no toca el archivo y no truena
S="$TMP/broken.json"
printf '{not json' > "$S"
CC_CLAUDE_SETTINGS="$S" cc_register_claude_hooks "$CMD" 2>/dev/null
[ "$(cat "$S")" = "{not json" ] || { echo "broken: invalid file was modified"; exit 1; }

# Case 5: un archivo existente pero vacio tambien es JSON invalido; no se
# interpreta como si no existiera ni se reemplaza silenciosamente.
S="$TMP/empty.json"
: > "$S"
CC_CLAUDE_SETTINGS="$S" cc_register_claude_hooks "$CMD" 2>/dev/null
[ ! -s "$S" ] || { echo "empty: existing empty file was modified"; exit 1; }

# Case 6: en WSL jq se instala dentro del dispatch de plataforma. El registro
# debe ocurrir despues para que una instalacion limpia no termine sin hooks.
deps_line=$(grep -n '^[[:space:]]*_cc_wsl_install_deps$' "$ROOT/install.sh" | cut -d: -f1)
register_line=$(grep -n '^cc_register_claude_hooks ' "$ROOT/install.sh" | cut -d: -f1)
[ -n "$deps_line" ] && [ -n "$register_line" ] && [ "$register_line" -gt "$deps_line" ] \
  || { echo "order: Claude hooks are registered before WSL dependencies"; exit 1; }

echo "test_install_hooks: OK"
