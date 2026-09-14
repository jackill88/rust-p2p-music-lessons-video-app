# Source this file before running Android build commands:
#   source android/env.sh
#
# Detects Android Studio (snap), SDK, NDK, and Java on this machine.

# Android SDK (Android Studio default on Linux).
if [[ -z "${ANDROID_HOME:-}" ]]; then
  if [[ -d "$HOME/Android/Sdk" ]]; then
    export ANDROID_HOME="$HOME/Android/Sdk"
  elif [[ -d "$HOME/.android/sdk" ]]; then
    export ANDROID_HOME="$HOME/.android/sdk"
  fi
fi
export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"

# JDK bundled with Android Studio, then a local Temurin/Adoptium install.
if [[ -z "${JAVA_HOME:-}" ]]; then
  if [[ -d "/snap/android-studio/current/jbr" ]]; then
    export JAVA_HOME="/snap/android-studio/current/jbr"
  elif [[ -d "/opt/android-studio/jbr" ]]; then
    export JAVA_HOME="/opt/android-studio/jbr"
  elif [[ -d "$HOME/android-studio/jbr" ]]; then
    export JAVA_HOME="$HOME/android-studio/jbr"
  elif [[ -d "$HOME/java/17.0.13+11" ]]; then
    export JAVA_HOME="$HOME/java/17.0.13+11"
  fi
fi

# Latest installed NDK under the SDK.
if [[ -z "${NDK_HOME:-}" && -n "${ANDROID_HOME:-}" && -d "$ANDROID_HOME/ndk" ]]; then
  export NDK_HOME="$(ls -1d "$ANDROID_HOME/ndk/"* 2>/dev/null | sort -V | tail -1)"
fi

if [[ -n "${JAVA_HOME:-}" ]]; then
  export PATH="$JAVA_HOME/bin:$PATH"
fi
if [[ -n "${ANDROID_HOME:-}" ]]; then
  export PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"
fi
