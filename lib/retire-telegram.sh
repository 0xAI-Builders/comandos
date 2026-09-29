#!/usr/bin/env bash
# N3: retira el bot de Telegram de una instalacion de ComandOS.
#
# Solo actua sobre lo que instalo ESTE producto: la unidad de usuario
# cc-telegram.service enlazada a <repo>/systemd/ y los enlaces a
# <repo>/bin/cc-telegram y <repo>/hooks/md2tg.py de un checkout de ComandOS.
# No borra telegram.env, tg-targets ni historial; no toca unidades ajenas.
# install.sh la llama solo con COMANDOS_RETIRE_TELEGRAM=1 (activacion en R3).

# $1 = enlace instalado, $2 = ruta relativa esperada dentro del repo
_cc_comandos_link() {
  [ -L "$1" ] || return 1
  local target root
  target=$(readlink "$1") || return 1
  case "$target" in */"$2") ;; *) return 1 ;; esac
  root="${target%/"$2"}"
  [ -e "$root/bin/cc-dash" ] && [ -e "$root/lib/platform.sh" ]
}

cc_retire_telegram() {
  local units="$HOME/.config/systemd/user" unit changed=0 pair link suffix
  unit="$units/cc-telegram.service"
  if _cc_comandos_link "$unit" "systemd/cc-telegram.service"; then
    systemctl --user disable --now cc-telegram.service >/dev/null 2>&1 || true
    rm -f "$unit"
    # disable ya quita el enlace de wants; si systemctl fallo, se quita el nuestro.
    link="$units/default.target.wants/cc-telegram.service"
    [ -L "$link" ] && [ "$(readlink "$link")" = "$unit" ] && rm -f "$link"
    changed=1
    echo "  Telegram retirado: cc-telegram.service detenido y deshabilitado."
  elif [ -e "$unit" ] || [ -L "$unit" ]; then
    echo "  (cc-telegram.service no es de ComandOS: no lo toco)"
  fi
  for pair in "$HOME/.local/bin/cc-telegram:bin/cc-telegram" \
              "$HOME/.claude/hooks/md2tg.py:hooks/md2tg.py"; do
    link="${pair%%:*}"; suffix="${pair#*:}"
    if _cc_comandos_link "$link" "$suffix"; then rm -f "$link"; fi
  done
  if [ "$changed" = "1" ]; then
    systemctl --user daemon-reload >/dev/null 2>&1 || true
    echo "  Tus credenciales (~/.claude/hooks/telegram.env) quedan intactas; bórralas si ya no las usas."
  fi
  return 0
}
