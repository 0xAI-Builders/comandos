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
