#!/bin/bash
# Compare policy diagnostics with a minimal, reviewed C bundle. Never launch it.
# A matching diagnostic does not clear Translay or explain a user's exact alert.
set -euo pipefail

if [ "$(/usr/bin/uname -s)" != Darwin ] || [ "$#" -ne 1 ]; then
  printf 'Usage on macOS: bash collect-policy-control.sh TARGET\n' >&2
  exit 2
fi
case "$1" in
  aarch64-apple-darwin) architecture=arm64 ;;
  x86_64-apple-darwin) architecture=x86_64 ;;
  *) printf 'Unsupported target: %s\n' "$1" >&2; exit 2 ;;
esac

scratch=$(/usr/bin/mktemp -d "${TMPDIR:-/tmp}/translay-policy-control.XXXXXX")
trap '/bin/rm -rf "$scratch"' EXIT
app="$scratch/Translay Policy Control.app"
/bin/mkdir -p "$app/Contents/MacOS"
/bin/cat > "$scratch/main.c" <<'SOURCE'
int main(void) { return 0; }
SOURCE
/bin/cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.malusry.translay.policy-control</string>
  <key>CFBundleName</key><string>Policy Control</string>
  <key>CFBundleExecutable</key><string>policy-control</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.1</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
</dict></plist>
PLIST

printf 'Minimal C app policy control; no Translay application code is linked.\n'
printf 'This executable is inspected only and is NEVER launched or distributed.\n'
printf 'Its policy results do not establish whether Translay is safe to run.\n'
printf 'Source commit: %s\n' "$(git rev-parse HEAD)"
printf 'Source: '
/bin/cat "$scratch/main.c"
/usr/bin/sw_vers
/usr/bin/uname -m
/usr/bin/clang --version
/usr/bin/clang -arch "$architecture" -mmacosx-version-min=12.0 \
  -O2 -Wall -Werror "$scratch/main.c" -o "$app/Contents/MacOS/policy-control"
/usr/bin/lipo "$app/Contents/MacOS/policy-control" -verify_arch "$architecture"
/usr/bin/codesign --force --sign - --options runtime "$app"
/usr/bin/codesign --verify --deep --strict --verbose=2 "$app"

assess() {
  local label="$1"
  local status=0
  shift
  printf '\n--- %s ---\n' "$label"
  "$@" 2>&1 || status=$?
  printf 'exit_status=%s\n' "$status"
}
assess 'Control signing identity' /usr/bin/codesign --display --verbose=4 "$app"
if [ -x /usr/bin/syspolicy_check ]; then
  assess 'Control pre-distribution policy' /usr/bin/syspolicy_check distribution "$app"
else
  printf '\nControl pre-distribution policy: tool unavailable; result unknown.\n'
fi
assess 'Control Gatekeeper assessment' /usr/sbin/spctl --assess --type execute --verbose=4 "$app"
printf '\nControl complete. Translay policy gates are unchanged; no app was launched.\n'
