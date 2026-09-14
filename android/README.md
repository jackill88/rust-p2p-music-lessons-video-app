# Android build

Build the Lesson Studio **client** as an Android APK using [Dioxus 0.7](https://dioxuslabs.com/) mobile support. The **server** still runs separately on Ubuntu or Windows; the phone connects over Wi-Fi using `ws://<server-ip>:44041/ws`.

## Prerequisites

On this machine:

- Android Studio with SDK at `~/Android/Sdk`
- Dioxus CLI (`dx`)
- Rust stable

One-time setup from the project root:

```bash
chmod +x android/*.sh android/lib/*.sh
./android/setup.sh
```

In **SDK Manager**, ensure these are installed:

- Android SDK Platform **API 35**
- Android SDK Build-Tools
- Android SDK Platform-Tools
- NDK (Side by side)

Create an AVD or connect a phone with USB debugging enabled.

## Quick start

Terminal 1 — studio server:

```bash
cargo run -p lesson-server
```

Terminal 2 — APK:

```bash
./android/build.sh
./android/install.sh
```

On the phone, open **Lesson Studio**, enter your name, and set the server IP to the PC’s LAN address (for example `192.168.1.10`), port `44041`. Do **not** use `127.0.0.1` on a physical device unless you use `adb reverse`.

Allow **camera** and **microphone** when Android asks. Headphones are recommended so the instrument audio stays clean.

## Scripts

| Script | Purpose |
|--------|---------|
| `env.sh` | Export `ANDROID_HOME`, `JAVA_HOME`, `NDK_HOME` |
| `setup.sh` | Verify toolchain and install Rust Android targets |
| `build.sh` | Build **arm64** debug APK → `android/dist/lesson-studio-debug.apk` |
| `build.sh --emulator` | Build **x86_64** APK for the emulator |
| `build.sh --release` | Signed arm64 release APK |
| `serve.sh` | `dx serve --android` with hot reload |
| `install.sh` | Install the latest APK via `adb` |

## USB testing against a server on the same PC

```bash
adb reverse tcp:44041 tcp:44041
```

Then use `127.0.0.1` and port `44041` in the app.

## Troubleshooting

**`JAVA_HOME is not set`** — Scripts look for `/snap/android-studio/current/jbr` and `~/java/17.0.13+11`. Set `JAVA_HOME` if your JDK is elsewhere.

**Cannot connect** — Use the host LAN IP, not `127.0.0.1`. Confirm the server log shows `0.0.0.0:44041` and TCP 44041 is allowed through the firewall.

**WebSocket fails on device** — `build.sh` patches `network_security_config.xml` so plain `ws://` works to LAN hosts.

**Camera / mic blocked** — The build patches `CAMERA`, `RECORD_AUDIO`, and `MODIFY_AUDIO_SETTINGS` into the generated manifest. Reinstall after `./android/build.sh`.

**App not compatible on a phone** — Rebuild for arm64 (default). Use `--emulator` only for an x86_64 AVD.
