#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# shellcheck source=/dev/null
. "$ROOT/lib/platform.sh"

CMD="$HOME/.claude/hooks/cc-notify.sh"
TOOL_CMD="$HOME/.claude/hooks/cc-usage-tool.sh"
EVENTS="UserPromptSubmit Stop Notification SessionEnd"
TOOL_EVENTS="PreToolUse PostToolUse"

count_cmd() { # $1=settings $2=evento $3=comando opcional
  local expected_cmd="${3:-$CMD}"
  jq --arg cmd "$expected_cmd" --arg ev "$2" \
    '[.hooks[$ev][]? | (.hooks // [])[] | select(.command == $cmd)] | length' "$1"
}

# Case 1: settings.json inexistente -> se crea con los 4 eventos
S="$TMP/fresh/settings.json"
CC_CLAUDE_SETTINGS="$S" cc_register_claude_hooks "$CMD"
for ev in $EVENTS; do
  n=$(count_cmd "$S" "$ev")
  [ "$n" = "1" ] || { echo "fresh: expected 1 hook for $ev, got $n"; exit 1; }
done
for ev in $TOOL_EVENTS; do
  n=$(count_cmd "$S" "$ev" "$TOOL_CMD")
  [ "$n" = "1" ] || { echo "fresh: expected 1 telemetry hook for $ev, got $n"; exit 1; }
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
cp "$S" "$S.before"
CC_CLAUDE_SETTINGS="$S" cc_register_claude_hooks "$CMD" 2>/dev/null
cmp -s "$S.before" "$S" || { echo "broken: invalid file was modified"; exit 1; }

# Case 5: un archivo existente pero vacio tambien es JSON invalido; no se
# interpreta como si no existiera ni se reemplaza silenciosamente.
S="$TMP/empty.json"
: > "$S"
cp "$S" "$S.before"
CC_CLAUDE_SETTINGS="$S" cc_register_claude_hooks "$CMD" 2>/dev/null
cmp -s "$S.before" "$S" || { echo "empty: existing empty file was modified"; exit 1; }

# Case 6: en WSL jq se instala dentro del dispatch de plataforma. El registro
# debe ocurrir despues para que una instalacion limpia no termine sin hooks.
deps_line=$(grep -n '^[[:space:]]*_cc_wsl_install_deps$' "$ROOT/install.sh" | cut -d: -f1)
register_line=$(grep -n '^cc_register_claude_hooks ' "$ROOT/install.sh" | cut -d: -f1)
[ -n "$deps_line" ] && [ -n "$register_line" ] && [ "$register_line" -gt "$deps_line" ] \
  || { echo "order: Claude hooks are registered before WSL dependencies"; exit 1; }

# Case 7: si el setup de servicios falla, los hooks ya deben estar escritos.
# Ejecuta el instalador en HOME aislado y hace fallar systemctl al llegar al
# dispatch linux-native; no toca servicios reales.
FAKE_HOME="$TMP/install-home"
FAKE_BIN="$TMP/install-bin"
mkdir -p "$FAKE_HOME" "$FAKE_BIN"
cat > "$FAKE_BIN/systemctl" <<'EOF'
#!/usr/bin/env bash
exit 1
EOF
chmod +x "$FAKE_BIN/systemctl"
if HOME="$FAKE_HOME" PATH="$FAKE_BIN:$PATH" \
    CC_MOCK_UNAME=Linux \
    CC_MOCK_OSRELEASE_FILE="$TMP/no-wsl-release" \
    CC_MOCK_OS_RELEASE_FILE="$TMP/no-os-release" \
    bash "$ROOT/install.sh" >/dev/null 2>&1; then
  echo "install failure: mocked systemctl unexpectedly succeeded"
  exit 1
fi
for ev in $EVENTS; do
  n=$(count_cmd "$FAKE_HOME/.claude/settings.json" "$ev" \
    "$FAKE_HOME/.claude/hooks/cc-notify.sh")
  [ "$n" = "1" ] \
    || { echo "install failure: hook missing for $ev after later setup error"; exit 1; }
done


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

# Case 8: los hooks propios llevan "timeout" (segundos) para que un hook
# colgado nunca frene al agente; se agrega tambien a entradas propias viejas
# sin timeout, respeta un timeout puesto a mano y no toca hooks ajenos.
timeout_of() { # $1=settings $2=evento $3=comando
  jq -r --arg cmd "$3" --arg ev "$2" \
    '[.hooks[$ev][]? | (.hooks // [])[] | select(.command == $cmd) | .timeout] | .[0] // "none"' "$1"
}
S="$TMP/timeout-fresh.json"
CC_CLAUDE_SETTINGS="$S" cc_register_claude_hooks "$CMD"
for ev in $EVENTS; do
  [ "$(timeout_of "$S" "$ev" "$CMD")" = "15" ] || { echo "timeout: $ev sin timeout 15"; exit 1; }
done
for ev in $TOOL_EVENTS; do
  [ "$(timeout_of "$S" "$ev" "$TOOL_CMD")" = "5" ] || { echo "timeout: $ev tool sin timeout 5"; exit 1; }
done
S="$TMP/timeout-old.json"
jq -n --arg cmd "$CMD" --arg tool "$TOOL_CMD" '{hooks:{
  Stop:[{hooks:[{type:"command",command:$cmd}]}],
  Notification:[{hooks:[{type:"command",command:$cmd,timeout:99}]}],
  PreToolUse:[{hooks:[{type:"command",command:"/otro/hook.sh"}]},{hooks:[{type:"command",command:$tool}]}]}}' > "$S"
CC_CLAUDE_SETTINGS="$S" cc_register_claude_hooks "$CMD"
[ "$(timeout_of "$S" Stop "$CMD")" = "15" ] || { echo "timeout: entrada vieja sin actualizar"; exit 1; }
[ "$(timeout_of "$S" Notification "$CMD")" = "99" ] || { echo "timeout: piso un timeout manual"; exit 1; }
[ "$(timeout_of "$S" PreToolUse "$TOOL_CMD")" = "5" ] || { echo "timeout: tool vieja sin timeout"; exit 1; }
[ "$(timeout_of "$S" PreToolUse /otro/hook.sh)" = "none" ] || { echo "timeout: toco un hook ajeno"; exit 1; }
[ "$(count_cmd "$S" Stop)" = "1" ] || { echo "timeout: duplico la entrada vieja"; exit 1; }

echo "test_install_hooks: OK"
