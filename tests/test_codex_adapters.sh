#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

export HOME="$TMP/home"
mkdir -p "$HOME/.claude/hooks"

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
