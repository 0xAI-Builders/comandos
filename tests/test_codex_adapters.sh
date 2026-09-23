#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

export HOME="$TMP/home"
mkdir -p "$HOME/.claude/hooks"

# tmux falso: los adaptadores nunca deben hablar con el servidor tmux real.
unset TMUX TMUX_PANE
mkdir -p "$TMP/stubs"
cat > "$TMP/stubs/tmux" <<'FAKE'
#!/usr/bin/env bash
if [ "$1" = "display-message" ]; then printf '%s\n' "${FAKE_TMUX_SESSION:-}"; exit 0; fi
exit 1
FAKE
chmod +x "$TMP/stubs/tmux"
export PATH="$TMP/stubs:$PATH"

cat > "$HOME/.claude/hooks/cc-notify.sh" <<'FAKE'
#!/usr/bin/env bash
python3 - "$@" <<'PY'
import json
import os
import sys

with open(os.path.join(os.environ["HOME"], "notify.jsonl"), "a") as f:
    f.write(json.dumps(sys.argv[1:]) + "\n")
PY
FAKE
chmod +x "$HOME/.claude/hooks/cc-notify.sh"

payload_user_prompt='{"hook_event_name":"UserPromptSubmit","cwd":"/tmp/proj","prompt":"hello"}'
printf '%s' "$payload_user_prompt" | "$ROOT/adapters/codex-hooks.sh"

python3 - "$HOME/notify.jsonl" <<'PY'
import json
import sys

args = json.loads(open(sys.argv[1]).read().splitlines()[-1])
assert args[:6] == ["--agent", "codex", "--event", "working", "--cwd", "/tmp/proj"], args
PY

payload_stop='{"hook_event_name":"Stop","cwd":"/tmp/proj","last_assistant_message":"final answer"}'
printf '%s' "$payload_stop" | "$ROOT/adapters/codex-hooks.sh"

python3 - "$HOME/notify.jsonl" <<'PY'
import json
import sys

args = json.loads(open(sys.argv[1]).read().splitlines()[-1])
assert args[:6] == ["--agent", "codex", "--event", "done", "--cwd", "/tmp/proj"], args
assert "--full" in args, args
assert args[args.index("--full") + 1] == "final answer", args
PY

payload_permission='{"hook_event_name":"PermissionRequest","cwd":"/tmp/proj","tool_name":"Bash","command":"npm install","reason":"needs network"}'
printf '%s' "$payload_permission" | "$ROOT/adapters/codex-hooks.sh"

python3 - "$HOME/notify.jsonl" <<'PY'
import json
import sys

args = json.loads(open(sys.argv[1]).read().splitlines()[-1])
assert args[:6] == ["--agent", "codex", "--event", "waiting", "--cwd", "/tmp/proj"], args
assert "--msg" in args, args
assert "permiso" in args[args.index("--msg") + 1].lower(), args
assert "--full" in args, args
assert "npm install" in args[args.index("--full") + 1], args
PY

mkdir -p "$HOME/.claude/hooks/state"
now="$(date +%s)"
printf '{"agent":"codex","status":"done","ts":%s}\n' "$now" > "$HOME/.claude/hooks/state/proj.json"
rm -f "$HOME/notify.jsonl"

legacy_payload='{"type":"agent-turn-complete","cwd":"/tmp/proj","last-assistant-message":"legacy done"}'
"$ROOT/adapters/codex-notify.sh" "$legacy_payload"

if [ -e "$HOME/notify.jsonl" ]; then
  echo "legacy notify duplicated a recent lifecycle Stop event" >&2
  exit 1
fi

printf '{"agent":"codex","status":"done","ts":0}\n' > "$HOME/.claude/hooks/state/proj.json"
"$ROOT/adapters/codex-notify.sh" "$legacy_payload"

python3 - "$HOME/notify.jsonl" <<'PY'
import json
import sys

args = json.loads(open(sys.argv[1]).read().splitlines()[-1])
assert args[:6] == ["--agent", "codex", "--event", "done", "--cwd", "/tmp/proj"], args
assert args[args.index("--full") + 1] == "legacy done", args
PY

# Bajo tmux cc-notify.sh escribe state/<proj>--<sesion>--<pane>.json: el dedupe
# de ambos adaptadores tiene que mirar ESA clave o Codex notifica dos veces.
rm -f "$HOME/.claude/hooks/state/proj.json" "$HOME/notify.jsonl"
now="$(date +%s)"
printf '{"agent":"codex","status":"done","ts":%s}\n' "$now" \
  > "$HOME/.claude/hooks/state/proj--mysess--7.json"
FAKE_TMUX_SESSION=mysess TMUX_PANE=%7 "$ROOT/adapters/codex-notify.sh" "$legacy_payload"
if [ -e "$HOME/notify.jsonl" ]; then
  echo "legacy notify ignoro el estado pane-aware y duplico la notificacion" >&2
  exit 1
fi
printf '%s' "$payload_stop" | FAKE_TMUX_SESSION=mysess TMUX_PANE=%7 "$ROOT/adapters/codex-hooks.sh"
if [ -e "$HOME/notify.jsonl" ]; then
  echo "codex-hooks Stop ignoro el estado pane-aware y duplico la notificacion" >&2
  exit 1
fi
# Otro pane de la misma sesion no hereda el dedupe
FAKE_TMUX_SESSION=mysess TMUX_PANE=%8 "$ROOT/adapters/codex-notify.sh" "$legacy_payload"
[ -e "$HOME/notify.jsonl" ] || { echo "el dedupe de un pane silencio a otro pane" >&2; exit 1; }
# Sin sesion tmux valida, cc-notify cae al nombre del proyecto: misma clave aqui
rm -f "$HOME/notify.jsonl"
printf '{"agent":"codex","status":"done","ts":%s}\n' "$now" \
  > "$HOME/.claude/hooks/state/proj--proj--9.json"
FAKE_TMUX_SESSION='' TMUX_PANE=%9 "$ROOT/adapters/codex-notify.sh" "$legacy_payload"
if [ -e "$HOME/notify.jsonl" ]; then
  echo "fallback de sesion distinto al de cc-notify.sh" >&2
  exit 1
fi
echo "test_codex_adapters: OK"
