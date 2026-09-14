#!/usr/bin/env bash
# Run the Lesson Studio client on a connected Android device or emulator.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

check_android_env

echo "Connected devices:"
adb devices -l
echo

cd "$CLIENT_DIR"
echo "Starting dx serve for Android (hot reload)..."
echo "Press Ctrl+C to stop."
dx serve --android
