#!/usr/bin/env bash
# Build a debug or release APK for the Lesson Studio client.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

RELEASE=0
FOR_EMULATOR=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      RELEASE=1
      shift
      ;;
    --emulator)
      FOR_EMULATOR=1
      shift
      ;;
    -h|--help)
      echo "Usage: $0 [--release] [--emulator]"
      echo
      echo "  (default)   Build arm64 APK for physical phones → android/dist/lesson-studio-debug.apk"
      echo "  --release   Release build (signed for adb install)"
      echo "  --emulator  Build x86_64 APK for the Android emulator instead"
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

check_android_env

if (( FOR_EMULATOR )); then
  ANDROID_TARGET="$ANDROID_EMULATOR_TARGET"
  echo "Target: $ANDROID_TARGET (Android emulator)"
else
  ANDROID_TARGET="$ANDROID_PHONE_TARGET"
  echo "Target: $ANDROID_TARGET (physical phones)"
fi

if (( RELEASE )); then
  PROFILE=release
  VARIANT=release
  GRADLE_TASK=assembleRelease
  DX_FLAGS=(--android --release --target "$ANDROID_TARGET")
else
  PROFILE=debug
  VARIANT=debug
  GRADLE_TASK=assembleDebug
  DX_FLAGS=(--android --target "$ANDROID_TARGET")
fi

echo "Building Rust + Android project (${PROFILE})..."
cd "$CLIENT_DIR"
dx build "${DX_FLAGS[@]}"

APP_DIR="$(find_android_app_dir "$PROFILE")"
patch_network_security_config "$APP_DIR"
patch_android_manifest "$APP_DIR"
if (( RELEASE )); then
  disable_release_lint "$APP_DIR"
fi

echo "Rebuilding APK with camera, microphone, and LAN WebSocket support..."
(cd "$APP_DIR" && ./gradlew "$GRADLE_TASK")

copy_apk_to_dist "$APP_DIR" "$VARIANT"
