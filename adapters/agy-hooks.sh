#!/usr/bin/env bash
# Antigravity CLI (agy) -> ComandOS. Registro global (cc-agents setup) en
# ~/.gemini/config/hooks.json con formato PLANO (sin wrapper {hooks:[]}):
#   "PreInvocation": [{"type":"command","command":".../agy-hooks.sh working"}]
#   "Stop":          [{"type":"command","command":".../agy-hooks.sh done"}]
# El payload de agy es camelCase, NO trae el nombre del evento (va como $1)
# y el directorio real es workspacePaths[0]. Salida: JSON vacio = no intervenir.
in=$(cat)
e="${1:-done}"
# Bind hook observations to an actual agy ancestor, never a latest-by-folder session.
printf '%s' "$in" | python3 -c '
import json,os,re,sys,tempfile,time
from pathlib import Path
try:
 data=json.load(sys.stdin);sid=data.get("conversationId","")
 if not re.fullmatch(r"[A-Za-z0-9_-]{1,256}",sid):raise ValueError()
 pid=os.getppid()
 for _ in range(10):
  proc=Path("/proc")/str(pid)
  argv=proc.joinpath("cmdline").read_bytes().split(b"\0")
  stat=proc.joinpath("stat").read_text().rsplit(")",1)[1].split()
  if argv and Path(os.fsdecode(argv[0])).name=="agy":break
  pid=int(stat[1])
 else:raise ValueError()
 state={"pid":pid,"start":stat[19],"harness":"agy","sessionId":sid,"parentId":"","busy":sys.argv[1]=="working","updatedAt":int(time.time()*1000)}
 model=data.get("modelName","")
 if isinstance(model,str) and re.fullmatch(r"[A-Za-z0-9_.:/-]{1,200}",model):state["model"]=model
 root=Path.home()/".claude/hooks/native-processes";root.mkdir(parents=True,exist_ok=True,mode=0o700)
 fd,temp=tempfile.mkstemp(dir=root,prefix=".agy-")
 with os.fdopen(fd,"w") as stream:json.dump(state,stream)
 os.replace(temp,root/(str(pid)+".json"))
except (OSError,ValueError,KeyError,TypeError,IndexError):pass
' "$e" >/dev/null 2>&1
cwd=$(jq -r '.workspacePaths[0] // ""' <<<"$in" 2>/dev/null)
cwd="${cwd:-$PWD}"
if [ -n "$cwd" ]; then
  "$HOME/.claude/hooks/cc-notify.sh" --agent agy --event "$e" --cwd "$cwd" >/dev/null 2>&1 &
fi
echo '{}'
exit 0
