# Lesson Studio

A Rust app for **piano and guitar lessons**: two devices share a camera view and high-quality audio. One machine runs the server; everyone else types that machine’s IP address.

- **Server** — standalone WebSocket relay for Ubuntu and Windows (default port **44041**)
- **Client** — Dioxus 0.7 UI for desktop, web, and Android

The published port is the whole rendezvous. There is no extra discovery service: start the server, open **44041/tcp**, and join from a PC or phone.

## How a lesson works

1. Launch `lesson-server` on the teacher PC, a studio PC, or a VPS.
2. Publish **TCP 44041** (LAN firewall or router port-forward).
3. Open Lesson Studio on two devices and enter the server IP (port stays `44041`).
4. The first two people are paired. The server relays WebRTC signaling; camera and audio go peer-to-peer (H.264 / Opus).

Use **headphones**. The mic is opened in music mode (no echo cancellation, noise suppression, or auto-gain) so piano and guitar keep their dynamics.

## Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Dioxus CLI](https://dioxuslabs.com/learn/0.7/) for the client:

  ```bash
  curl -fsSL https://dioxus.dev/install.sh | bash
  ```

### Linux desktop dependencies

The desktop client needs GTK and WebKit development libraries. On Ubuntu/Debian:

```bash
sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev
```

If you do not want to install these yet, run the client in the browser with `dx serve --web`.

Android Studio + SDK are enough for the phone build; see [`android/README.md`](android/README.md).

## Project layout

```
crates/
  protocol/  # join/chat/WebRTC signal messages
  server/    # lesson-server binary
  client/    # Dioxus UI (PC, web, Android)
android/     # APK scripts
```

## Launch the server (Ubuntu / Windows)

From the project root:

```bash
cargo run -p lesson-server --release
```

The server listens on **all interfaces** (`0.0.0.0`) on port **44041** using **HTTPS / WSS** (the Android app is an HTTPS page, so it cannot use plain `ws://`).

```bash
LESSON_PORT=44041 cargo run -p lesson-server --release
```

On Windows (PowerShell):

```powershell
$env:LESSON_PORT = "44041"
cargo run -p lesson-server --release
```

You should see log lines like:

```
Lesson Studio listening on https://0.0.0.0:44041
Publish TCP port 44041; clients connect with the server IP (WSS)
Reachable at https://192.168.1.10:44041  (wss://192.168.1.10:44041/ws)
```

The certificate is self-signed for LAN use. A health check from this PC:

```bash
curl -k https://127.0.0.1:44041/health
```

### Publish the port

Ubuntu:

```bash
sudo ufw allow 44041/tcp comment 'Lesson Studio'
```

Windows (admin PowerShell):

```powershell
netsh advfirewall firewall add rule name="Lesson Studio" dir=in action=allow protocol=TCP localport=44041
```

If the student is outside your LAN, forward **TCP 44041** on the router to the server PC, then share the public IP.

## Launch the client on Linux / PC

```bash
cd crates/client
dx serve --desktop
```

1. Enter your name, teacher/student, piano/guitar.
2. Enter the **server IP** (`127.0.0.1` if the server is on this PC).
3. Leave the port at **44041**.
4. Click **Join studio** and allow camera + microphone.

### Alternative: browser

```bash
cd crates/client
dx serve --web
```

Camera access in a browser only works on a secure origin (usually `http://127.0.0.1`). For a phone on Wi-Fi, install the Android app instead of opening the server page in Chrome.

## Playing a lesson

1. Start the server on a reachable host.
2. Join from two clients (PC + PC, PC + Android, or two phones).
3. The large view is the other person; your camera sits in the corner.
4. Flip camera on a phone to show the keyboard or fretboard.
5. Use the chat line for bar numbers and short notes.

A third client is rejected until someone leaves. If Leave does not reach the server (Wi-Fi change, sleep, etc.), join again with the **same name** — that reclaims your seat. Dropped sockets are also cleared after a few seconds of silence.

## Quick local test

Terminal 1:

```bash
cargo run -p lesson-server
```

Terminal 2 and 3 (or one desktop client and one `dx serve --web`):

```bash
cd crates/client
dx serve --desktop
```

Use `127.0.0.1` and port `44041`.

## Android

Build and install the APK with [`android/README.md`](android/README.md). On the phone, enter the **LAN IP of the server**, not `127.0.0.1`, unless you set up `adb reverse`.
