#!/bin/bash
# Inspect a bundle and macOS policy decisions without launching or modifying it.
set -u

if [ "$(/usr/bin/uname -s)" != Darwin ]; then
  printf 'This diagnostic requires macOS.\n' >&2
  exit 2
fi
if [ "$#" -gt 1 ]; then
  printf 'Usage: bash diagnose-macos.sh [path/to/Translay Mac Preview.app]\n' >&2
  exit 2
fi

app="${1:-/Applications/Translay Mac Preview.app}"
case "$app" in
  /*) ;;
  *) app="$PWD/$app" ;;
esac

check() {
  local label="$1"
  shift
  printf '\n--- %s ---\n' "$label"
  "$@" 2>&1
  local status=$?
  printf 'exit_status=%s\n' "$status"
  # A policy rejection is evidence to retain, not a reason to stop the report.
  return 0
}

printf 'Translay Mac startup diagnostic\n'
printf 'UTC: %s\n' "$(/bin/date -u '+%Y-%m-%dT%H:%M:%SZ')"
printf 'Bundle: %s\n' "$app"
printf 'This report does not launch the app, change signatures, remove quarantine,\n'
printf 'change security settings, or read Translay configuration/API keys.\n'
printf 'Review paths and log messages before sharing; they may contain your username.\n'
check 'macOS version' /usr/bin/sw_vers
check 'Machine architecture' /usr/bin/uname -m

if [ -d "$app" ]; then
  identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist" 2>/dev/null)
  if [ "$identifier" = com.malusry.translay.macos-spike ]; then
    binary="$app/Contents/MacOS/translay-macos-spike"
    check 'Bundle version' /usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist"
    check 'Executable architecture' /usr/bin/file "$binary"
    check 'Executable SHA-256' /usr/bin/shasum -a 256 "$binary"
    check 'Signing identity (not a safety assessment)' /usr/bin/codesign --display --verbose=4 "$app"
    check 'Signature integrity (not Gatekeeper acceptance)' /usr/bin/codesign --verify --deep --strict --verbose=2 "$app"
    check 'Quarantine metadata (read only)' /usr/bin/xattr -p com.apple.quarantine "$app"
    if [ -x /usr/bin/syspolicy_check ]; then
      check 'Pre-distribution system policy' /usr/bin/syspolicy_check distribution "$app"
    else
      printf '\nsyspolicy_check is unavailable on this macOS version.\n'
    fi
    check 'Gatekeeper assessment' /usr/sbin/spctl --assess --type execute --verbose=4 "$app"
  else
    printf '\nBundle identifier does not match the Translay Mac prototype; bundle checks skipped.\n'
  fi
else
  printf '\nBundle not found. No need to restore or launch a deleted app for this report.\n'
fi

# Restrict collection to messages mentioning our app, without requesting sudo.
check 'Recent Translay security messages (last 30 minutes; may be empty or restricted)' \
  /usr/bin/log show --last 30m --style compact --predicate \
  '(process == "syspolicyd" OR process == "amfid" OR process == "taskgated-helper" OR process CONTAINS[c] "XProtect") AND (eventMessage CONTAINS[c] "translay")'

printf '\nReport complete. A completed report does not mean the app is approved or safe.\n'
printf 'Do not override a warning that the app contains malware or will damage your computer.\n'
