#!/usr/bin/env bash
# Verifica la resolucion de agente/lanzamiento de bin/ccx a partir de
# ~/.claude/hooks/cc-notify.conf, sin tmux real: se inyecta un `tmux` falso
# en el PATH que registra sus argumentos (y hace fallar has-session para
# forzar la rama de creacion de sesion).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# HOME falso: aqui vive la config y el proyecto de prueba.
FAKE_HOME="$TMP/home"
mkdir -p "$FAKE_HOME/.claude/hooks"
PROJ="$TMP/proj-demo"
mkdir -p "$PROJ"

# tmux falso: registra argv en $TMUX_LOG; has-session falla (no existe la sesion).
BINDIR="$TMP/bin"
mkdir -p "$BINDIR"
cat > "$BINDIR/tmux" <<'EOF'
#!/usr/bin/env bash
if [ "${1:-}" = "has-session" ]; then exit 1; fi
{ printf 'ARGS_START\n'; for a in "$@"; do printf '%s\n' "$a"; done; printf 'ARGS_END\n'; } >> "$TMUX_LOG"
exit 0
EOF
chmod +x "$BINDIR/tmux"

run_ccx() {
  # $1 = ruta log tmux, resto = argumentos de ccx
  local log="$1"; shift
  : > "$log"
  env -u TMUX -u CCX_AGENT \
      HOME="$FAKE_HOME" TMUX_LOG="$log" PATH="$BINDIR:$PATH" \
      bash "$ROOT/bin/ccx" "$@" </dev/null
}

# --- Caso 1: AGENT_DEFAULT + AGENT_LAUNCH_<NOMBRE> para un wrapper propio ---
cat > "$FAKE_HOME/.claude/hooks/cc-notify.conf" <<EOF
AGENT_DEFAULT=claude-mpm
AGENT_LAUNCH_CLAUDE_MPM="echo MPM_MARKER; exec bash"
EOF
LOG1="$TMP/log1"
out1="$(run_ccx "$LOG1" "$PROJ")"

grep -q "MPM_MARKER" "$LOG1" \
  || { echo "FALLO 1: no se lanzo el comando personalizado (AGENT_LAUNCH_CLAUDE_MPM)"; cat "$LOG1"; exit 1; }
grep -q "con claude-mpm" <<<"$out1" \
  || { echo "FALLO 1b: AGENT_DEFAULT no se aplico ('$out1')"; exit 1; }

# --- Caso 2: -a manda sobre AGENT_DEFAULT y sobreescribe un builtin ---
cat > "$FAKE_HOME/.claude/hooks/cc-notify.conf" <<EOF
AGENT_DEFAULT=claude-mpm
AGENT_LAUNCH_CLAUDE="echo CLAUDE_OVERRIDE; exec bash"
EOF
LOG2="$TMP/log2"
out2="$(run_ccx "$LOG2" -a claude "$PROJ")"

grep -q "CLAUDE_OVERRIDE" "$LOG2" \
  || { echo "FALLO 2: AGENT_LAUNCH_CLAUDE no sobreescribio el builtin"; cat "$LOG2"; exit 1; }
grep -q "con claude" <<<"$out2" \
  || { echo "FALLO 2b: -a claude no gano sobre AGENT_DEFAULT ('$out2')"; exit 1; }

# --- Caso 3: sin config -> builtin claude por defecto ---
: > "$FAKE_HOME/.claude/hooks/cc-notify.conf"
LOG3="$TMP/log3"
out3="$(run_ccx "$LOG3" "$PROJ")"
grep -q "claude --continue" "$LOG3" \
  || { echo "FALLO 3: sin config no cayo al builtin claude"; cat "$LOG3"; exit 1; }

# --- Caso 4: una ruta de agente es una clave literal, no una regex de sed ---
LOG4="$TMP/log4"
ERR4="$TMP/err4"
run_ccx "$LOG4" -a /opt/custom-agent "$PROJ" 2>"$ERR4" >/dev/null
[ ! -s "$ERR4" ] \
  || { echo "FALLO 4: conf_get interpreto la ruta como regex"; cat "$ERR4"; exit 1; }
grep -q "/opt/custom-agent" "$LOG4" \
  || { echo "FALLO 4b: no se conservo el comando de agente"; cat "$LOG4"; exit 1; }

# --- Caso 5: el ejemplo documentado se puede descomentar y sourcear sin
# ejecutar el agente durante la carga de cc-notify.conf. ---
EXAMPLE_CONF="$TMP/example.conf"
sed -n 's/^# \(AGENT_LAUNCH_CLAUDE_MPM=.*\)$/\1/p' \
  "$ROOT/hooks/cc-notify.conf.example" > "$EXAMPLE_CONF"
example_value=$(bash -c 'set -u; . "$1"; printf "%s" "$AGENT_LAUNCH_CLAUDE_MPM"' \
  _ "$EXAMPLE_CONF")
[ "$example_value" = "claude-mpm run --continue 2>/dev/null || claude-mpm run" ] \
  || { echo "FALLO 5: el ejemplo AGENT_LAUNCH no es una asignacion segura"; exit 1; }

echo "test_ccx_agent.sh OK"
