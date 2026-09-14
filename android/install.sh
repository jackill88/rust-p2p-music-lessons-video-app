#!/usr/bin/env bash
# Install the latest built APK onto a connected device via adb.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

APK=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --apk)
      APK="$2"
      shift 2
      ;;
    -h|--help)
      echo "Usage: $0 [--apk PATH]"
      echo
      echo "Install android/dist/lesson-studio-debug.apk (or release if debug is missing)."
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

check_android_env

if [[ -z "$APK" ]]; then
  if [[ -f "$DIST_DIR/lesson-studio-debug.apk" ]]; then
    APK="$DIST_DIR/lesson-studio-debug.apk"
  elif [[ -f "$DIST_DIR/lesson-studio-release.apk" ]]; then
    APK="$DIST_DIR/lesson-studio-release.apk"
  else
    echo "error: no APK found in $DIST_DIR. Run ./android/build.sh first." >&2
    exit 1
  fi
fi

echo "Installing $APK ..."
adb install -r "$APK"
echo "Done."
