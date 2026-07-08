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
