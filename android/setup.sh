#!/usr/bin/env bash
# Verify Android toolchain and install Rust Android targets.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

echo "Checking Android build prerequisites..."
check_android_env

echo
echo "Environment:"
echo "  ANDROID_HOME=$ANDROID_HOME"
echo "  NDK_HOME=$NDK_HOME"
echo "  JAVA_HOME=$JAVA_HOME"
echo "  dx $(dx --version 2>/dev/null || echo 'unknown')"

echo
echo "Installing Rust Android targets..."
rustup target add aarch64-linux-android
rustup target add armv7-linux-androideabi
rustup target add x86_64-linux-android
rustup target add i686-linux-android

echo
echo "Checking adb..."
adb version | head -1

echo
echo "Setup complete. Next steps:"
echo "  ./android/build.sh          # build debug APK"
echo "  ./android/serve.sh          # run on emulator/device with hot reload"
echo "  ./android/install.sh        # install latest APK via adb"
