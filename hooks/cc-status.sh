#!/usr/bin/env bash
# Resumen para la barra de tmux: que proyectos piden atencion y cuales terminaron.
# Lo consume status-right en ~/.tmux.conf (se refresca cada 5s).
STATE="$HOME/.claude/hooks/state"
now=$(date +%s)
waiting=""
done_=""
for f in "$STATE"/*.json; do
  [ -e "$f" ] || continue
  line=$(jq -r '[.project,.status,(.ts|tostring)]|join("|")' "$f" 2>/dev/null)
  p=${line%%|*}; rest=${line#*|}; s=${rest%%|*}; t=${rest##*|}
  [ -z "$p" ] && continue
  age=$(( now - ${t:-0} ))
  # Ignorar estados viejos (mas de 8 horas)
  [ "$age" -gt 28800 ] && continue
  case "$s" in
    waiting) waiting="$waiting$p ";;
    done)    done_="$done_$p ";;
  esac
done
out=""
[ -n "$waiting" ] && out="#[fg=colour214,bold]ATENCION: ${waiting}#[default]"
[ -n "$done_" ]   && out="$out#[fg=colour84]LISTO: ${done_}#[default]"
printf '%s' "$out"
