#!/usr/bin/env bash
# ComandOS — detección de plataforma y helpers de instalación compartidos.
# Sourceable. No efectos secundarios al hacer source.

_cc_uname() {
  if [ -n "${CC_MOCK_UNAME:-}" ]; then echo "$CC_MOCK_UNAME"; else uname -s; fi
}

_cc_osrelease_file() {
  echo "${CC_MOCK_OSRELEASE_FILE:-/proc/sys/kernel/osrelease}"
}

_cc_os_release_file() {
  echo "${CC_MOCK_OS_RELEASE_FILE:-/etc/os-release}"
}

_cc_os_release_id() {
  local f
  f=$(_cc_os_release_file)
  [ -f "$f" ] || return 1
  # ID=ubuntu (unquoted per os-release spec, but tolerate quotes)
  awk -F= '/^ID=/ { gsub(/"/, "", $2); print $2; exit }' "$f"
}

cc_platform() {
  local sys
  sys=$(_cc_uname)
  case "$sys" in
    Darwin) echo "darwin"; return 0 ;;
    Linux)  ;;
    *)      echo "linux-other"; return 0 ;;
  esac
  local osrel_file is_wsl id
  osrel_file=$(_cc_osrelease_file)
  is_wsl=0
  if [ -r "$osrel_file" ] && grep -qi microsoft "$osrel_file"; then
    is_wsl=1
  fi
  id=$(_cc_os_release_id || true)
  if [ "$is_wsl" = "1" ]; then
    [ "$id" = "ubuntu" ] && { echo "linux-wsl-ubuntu"; return 0; }
    echo "linux-other"; return 0
  fi
  echo "linux-native"
}

cc_ubuntu_codename() {
  local f
  f=$(_cc_os_release_file)
  [ -f "$f" ] || return 0
  awk -F= '/^VERSION_CODENAME=/ { gsub(/"/, "", $2); print $2; exit }' "$f"
}

cc_systemd_ok() {
  local state
  if [ "${CC_MOCK_SYSTEMD_STATE+set}" = "set" ]; then
    state="${CC_MOCK_SYSTEMD_STATE}"
  else
    state=$(systemctl is-system-running 2>/dev/null || true)
  fi
  case "$state" in
    running|degraded) return 0 ;;
    *) return 1 ;;
  esac
}

ask_yn() {
  local prompt="${1:-¿Continuar?}"
  if [ "${CC_ASSUME_YES:-0}" = "1" ]; then return 0; fi
  local reply
  printf '%s [Y/n] ' "$prompt" >&2
  IFS= read -r reply || reply=""
  case "$reply" in
    ""|y|Y|yes|Yes|YES|si|Si|SI|sí|Sí) return 0 ;;
    *) return 1 ;;
  esac
}

# Which of the given packages are NOT installed. Test-mockable.
_cc_dpkg_missing() {
  if [ "${CC_MOCK_DPKG_MISSING+set}" = "set" ]; then
    local wanted="$*"
    for m in $CC_MOCK_DPKG_MISSING; do
      case " $wanted " in *" $m "*) echo "$m" ;; esac
    done
    return 0
  fi
  local pkg
  for pkg in "$@"; do
    dpkg -s "$pkg" >/dev/null 2>&1 || echo "$pkg"
  done
}

# Lista de paquetes apt que ComandOS necesita en Ubuntu WSL (por codename).
# Emite una advertencia a stderr si el codename no está probado.
cc_wsl_ubuntu_packages() {
  local codename webkit_pkg
  codename=$(cc_ubuntu_codename)
  case "$codename" in
    jammy) webkit_pkg="gir1.2-webkit2-4.0" ;;
    noble) webkit_pkg="gir1.2-webkit2-4.1" ;;
    *)
      echo "  (Ubuntu '$codename' no probado; skip WebKit)" >&2
      webkit_pkg=""
      ;;
  esac
  local base="tmux jq xclip wmctrl python3-gi gir1.2-gtk-3.0 gir1.2-vte-2.91 pulseaudio-utils wslu"
  if [ -n "$webkit_pkg" ]; then
    echo "$base $webkit_pkg"
  else
    echo "$base"
  fi
}

apt_install_confirmed() {
  local missing
  missing=$(_cc_dpkg_missing "$@" | tr '\n' ' ' | sed 's/[[:space:]]*$//')
  if [ -z "$missing" ]; then return 0; fi
  echo "Faltan paquetes: $missing" >&2
  ask_yn "¿Instalo ahora con sudo apt install?" || {
    echo "  Cancelado. Instala a mano:  sudo apt install $missing" >&2
    return 1
  }
  if [ "${CC_MOCK_APT:-0}" = "1" ]; then
    echo "[mock apt install $missing]"
    return 0
  fi
  sudo apt update && sudo apt install -y $missing
}

# Registra los hooks de Claude Code en ~/.claude/settings.json (idempotente).
# $1 = comando del hook (ruta a cc-notify.sh). Respeta hooks ajenos existentes
# y no duplica si el comando ya está registrado en ese evento.
# CC_CLAUDE_SETTINGS: override de la ruta para tests.
cc_register_claude_hooks() {
  local cmd="$1"
  local settings="${CC_CLAUDE_SETTINGS:-$HOME/.claude/settings.json}"
  command -v jq >/dev/null 2>&1 || {
    echo "  (sin jq: registra los hooks de Claude Code a mano en $settings)" >&2
    return 0
  }
  mkdir -p "$(dirname "$settings")"
  [ -e "$settings" ] || echo '{}' > "$settings"
  if ! jq -e . "$settings" >/dev/null 2>&1; then
    echo "  ($settings no es JSON válido; no toco los hooks — revísalo a mano)" >&2
    return 0
  fi
  local tmp
  tmp=$(mktemp)
  if jq --arg cmd "$cmd" '
      def ensure($ev):
        .hooks[$ev] = ((.hooks[$ev] // []) as $arr
          | if ($arr | map((.hooks // [])[] | .command // "") | index($cmd)) != null
            then $arr
            else $arr + [{"hooks": [{"type": "command", "command": $cmd}]}]
            end);
      .hooks //= {}
      | ensure("UserPromptSubmit") | ensure("Stop")
      | ensure("Notification")     | ensure("SessionEnd")
    ' "$settings" > "$tmp" 2>/dev/null; then
    mv "$tmp" "$settings"
  else
    rm -f "$tmp"
    echo "  (no pude actualizar $settings; registra los hooks a mano)" >&2
  fi
}
