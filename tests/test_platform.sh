#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# shellcheck source=/dev/null
. "$ROOT/lib/platform.sh"

# Case 1: macOS
result=$(CC_MOCK_UNAME=Darwin cc_platform)
[ "$result" = "darwin" ] || { echo "expected darwin, got $result"; exit 1; }

# Case 2: native Linux (Ubuntu, no microsoft in kernel)
echo "6.5.0-15-generic" > "$TMP/osrelease"
cat > "$TMP/os-release" <<'EOF'
ID=ubuntu
VERSION_CODENAME=jammy
EOF
result=$(CC_MOCK_UNAME=Linux \
         CC_MOCK_OSRELEASE_FILE="$TMP/osrelease" \
         CC_MOCK_OS_RELEASE_FILE="$TMP/os-release" \
         cc_platform)
[ "$result" = "linux-native" ] || { echo "expected linux-native, got $result"; exit 1; }

# Case 3: WSL Ubuntu (kernel has microsoft)
echo "5.15.153.1-microsoft-standard-WSL2" > "$TMP/osrelease"
result=$(CC_MOCK_UNAME=Linux \
         CC_MOCK_OSRELEASE_FILE="$TMP/osrelease" \
         CC_MOCK_OS_RELEASE_FILE="$TMP/os-release" \
         cc_platform)
[ "$result" = "linux-wsl-ubuntu" ] || { echo "expected linux-wsl-ubuntu, got $result"; exit 1; }

# Case 4: WSL non-Ubuntu (kernel has microsoft, os-release ID != ubuntu)
cat > "$TMP/os-release" <<'EOF'
ID=kali
VERSION_CODENAME=kali-rolling
EOF
result=$(CC_MOCK_UNAME=Linux \
         CC_MOCK_OSRELEASE_FILE="$TMP/osrelease" \
         CC_MOCK_OS_RELEASE_FILE="$TMP/os-release" \
         cc_platform)
[ "$result" = "linux-other" ] || { echo "expected linux-other, got $result"; exit 1; }

# cc_ubuntu_codename
cat > "$TMP/os-release" <<'EOF'
ID=ubuntu
VERSION_CODENAME=jammy
EOF
result=$(CC_MOCK_OS_RELEASE_FILE="$TMP/os-release" cc_ubuntu_codename)
[ "$result" = "jammy" ] || { echo "expected jammy, got $result"; exit 1; }

cat > "$TMP/os-release" <<'EOF'
ID=ubuntu
VERSION_CODENAME=noble
EOF
result=$(CC_MOCK_OS_RELEASE_FILE="$TMP/os-release" cc_ubuntu_codename)
[ "$result" = "noble" ] || { echo "expected noble, got $result"; exit 1; }

# Missing os-release → empty
result=$(CC_MOCK_OS_RELEASE_FILE="$TMP/nonexistent" cc_ubuntu_codename || true)
[ -z "$result" ] || { echo "expected empty, got $result"; exit 1; }

# cc_systemd_ok
CC_MOCK_SYSTEMD_STATE=running   cc_systemd_ok || { echo "running should be ok"; exit 1; }
CC_MOCK_SYSTEMD_STATE=degraded  cc_systemd_ok || { echo "degraded should be ok"; exit 1; }
CC_MOCK_SYSTEMD_STATE=offline   cc_systemd_ok && { echo "offline should NOT be ok"; exit 1; } || true
CC_MOCK_SYSTEMD_STATE=""        cc_systemd_ok && { echo "empty should NOT be ok"; exit 1; } || true

# ask_yn — CC_ASSUME_YES bypass
CC_ASSUME_YES=1 ask_yn "test?" || { echo "ASSUME_YES should return 0"; exit 1; }

# ask_yn — piped input
echo "y" | ask_yn "test?" || { echo "y should return 0"; exit 1; }
echo ""  | ask_yn "test?" || { echo "empty (default Y) should return 0"; exit 1; }
echo "n" | ask_yn "test?" && { echo "n should return 1"; exit 1; } || true

# apt_install_confirmed — mock mode: prints intended install, no real apt
out=$(CC_MOCK_APT=1 CC_ASSUME_YES=1 CC_MOCK_DPKG_MISSING="foo bar" \
      apt_install_confirmed foo bar baz 2>&1)
echo "$out" | grep -q "\[mock apt install foo bar\]" \
  || { echo "expected mock install of foo bar, got: $out"; exit 1; }

# apt_install_confirmed — nothing missing → returns 0 without prompt
out=$(CC_MOCK_APT=1 CC_MOCK_DPKG_MISSING="" apt_install_confirmed foo bar 2>&1)
echo "$out" | grep -q "\[mock apt install" \
  && { echo "should not have run apt when nothing missing"; exit 1; } || true

# apt_install_confirmed — declined
out=$(CC_MOCK_APT=1 CC_MOCK_DPKG_MISSING="foo" apt_install_confirmed foo <<< "n" 2>&1) && \
  { echo "declined install should return 1"; exit 1; } || true

# cc_wsl_ubuntu_packages — jammy
cat > "$TMP/os-release" <<'EOF'
ID=ubuntu
VERSION_CODENAME=jammy
EOF
result=$(CC_MOCK_OS_RELEASE_FILE="$TMP/os-release" cc_wsl_ubuntu_packages)
echo "$result" | grep -q "gir1.2-webkit2-4.0" || { echo "jammy should have webkit2-4.0"; exit 1; }
echo "$result" | grep -q "gir1.2-webkit2-4.1" && { echo "jammy should NOT have webkit2-4.1"; exit 1; } || true

# noble
cat > "$TMP/os-release" <<'EOF'
ID=ubuntu
VERSION_CODENAME=noble
EOF
result=$(CC_MOCK_OS_RELEASE_FILE="$TMP/os-release" cc_wsl_ubuntu_packages)
echo "$result" | grep -q "gir1.2-webkit2-4.1" || { echo "noble should have webkit2-4.1"; exit 1; }

echo "ok"
