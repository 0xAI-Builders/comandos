#!/usr/bin/env bash
# Resumen para la barra de tmux: que proyectos piden atencion y cuales terminaron.
# Lo consume status-right en ~/.tmux.conf (se refresca cada 5s).
STATE="$HOME/.claude/hooks/state"
now=$(date +%s)
waiting=""
done_=""
files=("$STATE"/*.json)
[ -e "${files[0]}" ] || { printf ''; exit 0; }
while IFS= read -r -d '' line; do
  p=${line%%|*}; rest=${line#*|}; s=${rest%%|*}; t=${rest##*|}
  [ -z "$p" ] && continue
  age=$(( now - ${t:-0} ))
  # Ignorar estados viejos (mas de 8 horas)
  [ "$age" -gt 28800 ] && continue
  case "$s" in
    waiting) waiting="$waiting$p ";;
    done)    done_="$done_$p ";;
  esac
done < <(
  for f in "${files[@]}"; do
    [ -f "$f" ] || continue
    json=$(<"$f") 2>/dev/null || continue
    printf '%s\0' "$json"
  done |
    jq -Rrjs '
      split("\u0000")[]
      | fromjson?
      | select(type == "object")
      | [.project, .status, (.ts | tostring)]
      | join("|") + "\u0000"
    ' 2>/dev/null
)
out=""
[ -n "$waiting" ] && out="#[fg=colour214,bold]ATENCION: ${waiting}#[default]"
[ -n "$done_" ]   && out="$out#[fg=colour84]LISTO: ${done_}#[default]"
printf '%s' "$out"
