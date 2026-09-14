#!/usr/bin/env bash
# Shared helpers for Android build scripts.

set -euo pipefail

ANDROID_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT_ROOT="$(cd "$ANDROID_DIR/.." && pwd)"
CLIENT_DIR="$PROJECT_ROOT/crates/client"
DIST_DIR="$ANDROID_DIR/dist"
NETWORK_CONFIG="$ANDROID_DIR/res/xml/network_security_config.xml"

source "$ANDROID_DIR/env.sh"

ANDROID_PHONE_TARGET="${ANDROID_PHONE_TARGET:-aarch64-linux-android}"
ANDROID_EMULATOR_TARGET="${ANDROID_EMULATOR_TARGET:-x86_64-linux-android}"
CLIENT_PACKAGE_DIR="lesson-client"

require_cmd() {
  local name="$1"
  if ! command -v "$name" >/dev/null 2>&1; then
    echo "error: '$name' not found in PATH" >&2
    exit 1
  fi
}

check_android_env() {
  local missing=0

  require_cmd dx
  require_cmd rustup
  require_cmd adb

  if [[ -z "${ANDROID_HOME:-}" || ! -d "$ANDROID_HOME" ]]; then
    echo "error: ANDROID_HOME is not set and ~/Android/Sdk was not found." >&2
    missing=1
  fi

  if [[ -z "${JAVA_HOME:-}" || ! -x "${JAVA_HOME}/bin/java" ]]; then
    echo "error: JAVA_HOME is not set. Install Android Studio or set JAVA_HOME to a JDK." >&2
    missing=1
  fi

  if [[ -z "${NDK_HOME:-}" || ! -d "$NDK_HOME" ]]; then
    echo "error: NDK_HOME is not set. Install the NDK in Android Studio (SDK Manager)." >&2
    missing=1
  fi

  if (( missing )); then
    exit 1
  fi
}

get_target_dir() {
  cargo metadata --manifest-path "$CLIENT_DIR/Cargo.toml" --format-version 1 \
    | python3 -c "import json, sys; print(json.load(sys.stdin)['target_directory'])"
}

find_android_app_dir() {
  local profile="${1:-release}"
  local target_dir
  target_dir="$(get_target_dir)"
  local candidate="$target_dir/dx/$CLIENT_PACKAGE_DIR/$profile/android/app"

  if [[ -x "$candidate/gradlew" ]]; then
    echo "$candidate"
    return 0
  fi

  local found
  found="$(find "$target_dir/dx/$CLIENT_PACKAGE_DIR" -path "*/android/app/gradlew" -print -quit 2>/dev/null || true)"
  if [[ -n "$found" ]]; then
    dirname "$found"
    return 0
  fi

  echo "error: generated Android project not found under $target_dir/dx/$CLIENT_PACKAGE_DIR/" >&2
  echo "Run ./android/build.sh first." >&2
  exit 1
}

patch_network_security_config() {
  local app_dir="$1"
  local dest="$app_dir/app/src/main/res/xml/network_security_config.xml"

  if [[ ! -f "$NETWORK_CONFIG" ]]; then
    echo "error: missing $NETWORK_CONFIG" >&2
    exit 1
  fi

  mkdir -p "$(dirname "$dest")"
  cp "$NETWORK_CONFIG" "$dest"
  echo "Patched network security config for LAN ws:// connections."
}

patch_android_manifest() {
  local app_dir="$1"
  local manifest="$app_dir/app/src/main/AndroidManifest.xml"

  if [[ ! -f "$manifest" ]]; then
    echo "error: missing $manifest" >&2
    exit 1
  fi

  ensure_manifest_permission "$manifest" "android.permission.INTERNET"
  ensure_manifest_permission "$manifest" "android.permission.ACCESS_NETWORK_STATE"
  ensure_manifest_permission "$manifest" "android.permission.CAMERA"
  ensure_manifest_permission "$manifest" "android.permission.RECORD_AUDIO"
  ensure_manifest_permission "$manifest" "android.permission.MODIFY_AUDIO_SETTINGS"
  ensure_manifest_feature "$manifest" "android.hardware.camera"
  ensure_manifest_feature "$manifest" "android.hardware.microphone"

  if grep -q "android:usesCleartextTraffic" "$manifest"; then
    sed -i 's/android:usesCleartextTraffic="false"/android:usesCleartextTraffic="true"/' "$manifest"
  fi

  python3 - "$manifest" <<'PY'
from pathlib import Path
import re
import sys

path = Path(sys.argv[1])
text = path.read_text()
# Plugging in a USB charger often fires keyboard/uiMode/density config
# changes. Without these flags Android recreates MainActivity and the
# WebView lesson (camera + WebRTC) looks like a full app reset.
config = (
    "orientation|screenLayout|screenSize|smallestScreenSize|keyboardHidden|"
    "keyboard|navigation|uiMode|density|fontScale|layoutDirection|locale|colorMode"
)
if 'android:configChanges="' in text:
    text = re.sub(
        r'android:configChanges="[^"]*"',
        f'android:configChanges="{config}"',
        text,
        count=1,
    )
elif "<activity " in text:
    text = text.replace(
        "<activity ",
        f'<activity android:configChanges="{config}" ',
        1,
    )
if "android:launchMode=" not in text and "<activity " in text:
    text = text.replace("<activity ", '<activity android:launchMode="singleTask" ', 1)
path.write_text(text)
PY

  echo "Patched AndroidManifest.xml with camera and microphone permissions."
}

patch_webview_ssl() {
  local app_dir="$1"
  local file
  file="$(find "$app_dir" -name 'RustWebViewClient.kt' -print -quit 2>/dev/null || true)"
  if [[ -z "$file" ]]; then
    echo "warning: RustWebViewClient.kt not found; self-signed wss:// may fail on the phone."
    return 0
  fi
  if grep -q "onReceivedSslError" "$file"; then
    echo "WebView already accepts the studio TLS certificate."
    return 0
  fi
  python3 - "$file" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
text = path.read_text()
if "import android.net.http.SslError" not in text:
    text = text.replace("import android.net.Uri\n", "import android.net.Uri\nimport android.net.http.SslError\n")
method = '''
    override fun onReceivedSslError(
        view: WebView?,
        handler: SslErrorHandler?,
        error: SslError?
    ) {
        // Lesson Studio speaks WSS with a LAN self-signed certificate.
        handler?.proceed()
    }

'''
needle = "    companion object {"
if needle not in text:
    raise SystemExit(f"could not patch {path}: companion object not found")
path.write_text(text.replace(needle, method + needle, 1))
PY
  echo "Patched WebView to accept the studio TLS certificate (wss://)."
}

ensure_manifest_permission() {
  local manifest="$1"
  local permission="$2"
  if grep -q "android:name=\"$permission\"" "$manifest"; then
    return 0
  fi
  python3 - "$manifest" "$permission" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
permission = sys.argv[2]
text = path.read_text()
needle = "    <application"
line = f'    <uses-permission android:name="{permission}" />\n'
if needle in text:
    text = text.replace(needle, line + needle, 1)
else:
    text = text.replace("</manifest>", line + "</manifest>", 1)
path.write_text(text)
PY
}

ensure_manifest_feature() {
  local manifest="$1"
  local feature="$2"
  if grep -q "android:name=\"$feature\"" "$manifest"; then
    return 0
  fi
  python3 - "$manifest" "$feature" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
feature = sys.argv[2]
text = path.read_text()
needle = "    <application"
line = f'    <uses-feature android:name="{feature}" android:required="false" />\n'
if needle in text:
    text = text.replace(needle, line + needle, 1)
else:
    text = text.replace("</manifest>", line + "</manifest>", 1)
path.write_text(text)
PY
}

disable_release_lint() {
  local app_dir="$1"
  local gradle="$app_dir/app/build.gradle.kts"

  if [[ ! -f "$gradle" ]]; then
    return 0
  fi

  if grep -q "checkReleaseBuilds = false" "$gradle"; then
    return 0
  fi

  sed -i '/sourceSets {/,/^    }$/{
    /^    }$/a\
    lint {\
        checkReleaseBuilds = false\
    }
  }' "$gradle"

  echo "Disabled release lint checks in Gradle (AGP/SDK lint crash workaround)."
}

find_apksigner() {
  local apksigner
  apksigner="$(find "${ANDROID_HOME}/build-tools" -name apksigner -type f 2>/dev/null | sort -V | tail -1 || true)"
  if [[ -z "$apksigner" || ! -x "$apksigner" ]]; then
    echo "error: apksigner not found under \$ANDROID_HOME/build-tools" >&2
    exit 1
  fi
  echo "$apksigner"
}

sign_release_apk() {
  local apk="$1"
  local keystore="${ANDROID_DEBUG_KEYSTORE:-$HOME/.android/debug.keystore}"
  local apksigner signed

  if [[ ! -f "$keystore" ]]; then
    echo "error: debug keystore not found at $keystore" >&2
    echo "Run any Android debug build once in Android Studio to create it." >&2
    exit 1
  fi

  apksigner="$(find_apksigner)"
  signed="${apk%.apk}.signed.apk"
  "$apksigner" sign \
    --ks "$keystore" \
    --ks-key-alias androiddebugkey \
    --ks-pass pass:android \
    --key-pass pass:android \
    --out "$signed" \
    "$apk"
  mv "$signed" "$apk"
  echo "Signed release APK with the Android debug keystore (for local adb install)."
}

verify_apk_abis() {
  local apk="$1"
  local aapt2 abis
  aapt2="$(find "${ANDROID_HOME}/build-tools" -name aapt2 -type f 2>/dev/null | sort -V | tail -1 || true)"
  if [[ -z "$aapt2" ]]; then
    return 0
  fi
  abis="$("$aapt2" dump badging "$apk" 2>/dev/null | sed -n "s/native-code: '//p" | tr -d "'")"
  echo "APK native ABIs: ${abis:-unknown}"
}

copy_apk_to_dist() {
  local app_dir="$1"
  local variant="$2"
  local apk

  apk="$(find "$app_dir/app/build/outputs/apk/$variant" -name "*.apk" -print -quit 2>/dev/null || true)"
  if [[ -z "$apk" ]]; then
    apk="$(find "$app_dir/app/build/outputs/apk" -name "*-${variant}*.apk" -print -quit 2>/dev/null || true)"
  fi
  if [[ -z "$apk" ]]; then
    echo "error: could not find ${variant} APK under $app_dir/app/build/outputs/apk" >&2
    exit 1
  fi

  mkdir -p "$DIST_DIR"
  local out_name="lesson-studio-${variant}.apk"
  cp "$apk" "$DIST_DIR/$out_name"
  if [[ "$variant" == "release" ]]; then
    sign_release_apk "$DIST_DIR/$out_name"
  fi
  verify_apk_abis "$DIST_DIR/$out_name"
  echo "APK: $DIST_DIR/$out_name"
}
