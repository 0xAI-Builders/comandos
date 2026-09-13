#!/bin/sh
set -eu
if [ "$(id -u)" -ne 0 ]; then
  echo "Run this installer with sudo." >&2
  exit 1
fi
source_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
profile_source="$source_dir/comandos-browser-chrome"
profile_target=/etc/apparmor.d/comandos-browser-chrome
if [ -e "$profile_target" ] && ! cmp -s "$profile_source" "$profile_target"; then
  echo "Refusing to replace a different existing $profile_target" >&2
  exit 1
fi
/usr/sbin/apparmor_parser --skip-kernel-load --skip-cache "$profile_source"
install -o root -g root -m 0644 "$profile_source" "$profile_target"
/usr/sbin/apparmor_parser --replace --skip-cache "$profile_target"
echo "Loaded comandos-browser-chrome for the pinned Chrome executable."
