#!/bin/bash
# Fail CI for a broken signature, wrong bundle/CPU, or a damaged ZIP round trip.
# This checks integrity only; it does not approve execution or scan for malware.
set -euo pipefail

if [ "$(/usr/bin/uname -s)" != Darwin ] || [ "$#" -ne 3 ]; then
  printf 'Usage on macOS: bash verify-package.sh APP ZIP TARGET\n' >&2
  exit 2
fi
app="$1"
archive="$2"
target="$3"
case "$target" in
  aarch64-apple-darwin) architecture=arm64 ;;
  x86_64-apple-darwin) architecture=x86_64 ;;
  *) printf 'Unsupported target: %s\n' "$target" >&2; exit 2 ;;
esac

verify_bundle() {
  local bundle="$1"
  local binary="$bundle/Contents/MacOS/translay-macos-spike"
  local identifier
  identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$bundle/Contents/Info.plist")
  if [ "$identifier" != com.malusry.translay.macos-spike ]; then
    printf 'FAIL: Unexpected bundle identifier.\n' >&2
    return 1
  fi
  if [ ! -f "$binary" ] || [ ! -x "$binary" ]; then
    printf 'FAIL: Main executable is missing or has lost executable permissions.\n' >&2
    return 1
  fi
  if ! /usr/bin/lipo -verify_arch "$architecture" "$binary"; then
    printf 'FAIL: Bundle does not contain the requested CPU architecture.\n' >&2
    return 1
  fi
  if ! /usr/bin/codesign --verify --deep --strict --verbose=2 "$bundle"; then
    printf 'FAIL: Bundle signature integrity is invalid.\n' >&2
    return 1
  fi
}

printf 'Translay Mac package integrity verification\n'
printf 'Source commit: %s\n' "$(git rev-parse HEAD)"
printf 'Target: %s\n' "$target"
/usr/bin/sw_vers
printf '\nChecking the bundle before archiving...\n'
verify_bundle "$app"

scratch=$(/usr/bin/mktemp -d "${TMPDIR:-/tmp}/translay-package-check.XXXXXX")
trap '/bin/rm -rf "$scratch"' EXIT
/usr/bin/ditto -x -k "$archive" "$scratch"
restored="$scratch/$(/usr/bin/basename "$app")"
printf '\nChecking the extracted bundle...\n'
verify_bundle "$restored"
if ! /usr/bin/diff -rq "$app" "$restored"; then
  printf 'FAIL: Bundle contents changed in the ZIP round trip.\n' >&2
  exit 1
fi

printf '\nExecutable SHA-256:\n'
/usr/bin/shasum -a 256 "$restored/Contents/MacOS/translay-macos-spike"
printf '\nPASS: identity, CPU, signature integrity, executable permissions and ZIP round trip.\n'
printf 'This is NOT Gatekeeper acceptance, notarization, malware clearance, or a launch test.\n'
