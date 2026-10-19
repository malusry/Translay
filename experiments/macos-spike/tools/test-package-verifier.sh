#!/bin/bash
# Exercise rejection paths using disposable copies of the actual signed bundle.
set -euo pipefail

if [ "$(/usr/bin/uname -s)" != Darwin ] || [ "$#" -ne 3 ]; then
  printf 'Usage on macOS: bash test-package-verifier.sh APP ZIP TARGET\n' >&2
  exit 2
fi
app="$1"
archive="$2"
target="$3"
tools_dir="$(cd "$(dirname "$0")" && pwd)"
scratch=$(/usr/bin/mktemp -d "${TMPDIR:-/tmp}/translay-verifier-tests.XXXXXX")
trap '/bin/rm -rf "$scratch"' EXIT

# Establish that the input and the tools work before testing failure cases.
bash "$tools_dir/verify-package.sh" "$app" "$archive" "$target" > "$scratch/valid.txt" 2>&1

expect_rejection() {
  local reason="$1"
  shift
  if bash "$tools_dir/verify-package.sh" "$@" > "$scratch/rejection.txt" 2>&1; then
    printf 'FAIL: An invalid package was accepted: %s\n' "$reason" >&2
    exit 1
  fi
  if ! /usr/bin/grep -F "$reason" "$scratch/rejection.txt"; then
    /bin/cat "$scratch/rejection.txt" >&2
    exit 1
  fi
}

case "$target" in
  aarch64-apple-darwin) other_target=x86_64-apple-darwin ;;
  x86_64-apple-darwin) other_target=aarch64-apple-darwin ;;
  *) exit 2 ;;
esac
expect_rejection 'requested CPU architecture' "$app" "$archive" "$other_target"

copy="$scratch/Translay Mac Preview.app"
/usr/bin/ditto "$app" "$copy"
/bin/chmod a-x "$copy/Contents/MacOS/translay-macos-spike"
expect_rejection 'lost executable permissions' "$copy" "$archive" "$target"
/bin/chmod u+x "$copy/Contents/MacOS/translay-macos-spike"

# Changing a sealed file must fail both directly and after extraction.
/usr/libexec/PlistBuddy -c 'Set :CFBundleVersion 99999' "$copy/Contents/Info.plist"
expect_rejection 'signature integrity is invalid' "$copy" "$archive" "$target"
bad_archive="$scratch/modified.zip"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$copy" "$bad_archive"
expect_rejection 'signature integrity is invalid' "$app" "$bad_archive" "$target"

# A missing app must still produce a useful report, but must never pass the
# distribution gate. Neither path may try to launch the absent application.
bash "$tools_dir/diagnose-macos.sh" "$scratch/missing.app" > "$scratch/missing-report.txt" 2>&1
if bash "$tools_dir/diagnose-macos.sh" --require-policy-acceptance "$scratch/missing.app" > "$scratch/missing-gate.txt" 2>&1; then
  printf 'FAIL: Missing app passed system policy acceptance.\n' >&2
  exit 1
fi
/usr/bin/grep -F 'DISTRIBUTION BLOCKED' "$scratch/missing-gate.txt"

printf 'PASS: valid package accepted; wrong CPU, missing executable permissions,\n'
printf 'modified signature and modified archived bundle rejected; missing app cannot pass policy.\n'
