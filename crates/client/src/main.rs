use dioxus::prelude::*;
use lesson_protocol::{ClientMessage, Instrument, PeerInfo, Role, DEFAULT_PORT};
use serde::{Deserialize, Serialize};

const MAIN_CSS: Asset = asset!("/assets/main.css");
const SESSION_JS: &str = include_str!("../assets/lesson_session.js");

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Lobby,
    Lesson,
}

fn default_host() -> String {
    #[cfg(target_os = "android")]
    {
        String::new()
    }
    #[cfg(not(target_os = "android"))]
    {
        "127.0.0.1".into()
    }
}

fn facing_for(role: Role) -> &'static str {
    match role {
        Role::Teacher => "user",
        Role::Student => "environment",
    }
}

pub fn build_server_url(host: &str, port: &str) -> Result<String, String> {
    let host = host.trim();
    let port = port.trim();

    if host.starts_with("ws://") || host.starts_with("wss://") {
        let url = host.trim_end_matches('/');
        let url = url.replacen("ws://", "wss://", 1);
        return Ok(if url.ends_with("/ws") {
            url
        } else {
            format!("{url}/ws")
        });
    }

    if host.is_empty() {
        return Err("Enter the server IP address".into());
    }
    if port.is_empty() {
        return Err("Enter the server port".into());
    }
    let port: u16 = port.parse().map_err(|_| format!("Invalid port: {port}"))?;
    Ok(format!("wss://{host}:{port}/ws"))
}

#[derive(Serialize)]
struct ConnectCmd {
    op: &'static str,
    url: String,
    join: ClientMessage,
    facing: &'static str,
}

#[derive(Clone, Deserialize, Default)]
struct BridgeEvent {
    #[serde(default)]
    event: String,
    #[serde(default, rename = "type")]
    msg_type: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    partner: Option<PeerInfo>,
    #[serde(default)]
    you: Option<PeerInfo>,
    #[serde(default)]
    from: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    local: f32,
    #[serde(default)]
    remote: f32,
    #[serde(default)]
    kbps: u32,
    #[serde(default)]
    fps: u32,
}

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut screen = use_signal(|| Screen::Lobby);
    let name = use_signal(String::new);
    let role = use_signal(|| Role::Teacher);
    let instrument = use_signal(|| Instrument::Piano);
    let server_host = use_signal(default_host);
    let server_port = use_signal(|| DEFAULT_PORT.to_string());
    let mut error = use_signal(String::new);
    let mut status = use_signal(|| "Waiting to connect.".to_string());
    let mut partner = use_signal(|| None::<PeerInfo>);
    let mut you = use_signal(|| None::<PeerInfo>);
    let mut muted = use_signal(|| false);
    let mut camera_on = use_signal(|| true);
    let local_level = use_signal(|| 0.0f32);
    let remote_level = use_signal(|| 0.0f32);
    let kbps = use_signal(|| 0u32);
    let fps = use_signal(|| 0u32);
    let mut chat_input = use_signal(String::new);
    let mut chat_log = use_signal(Vec::<(String, String)>::new);
    let mut facing = use_signal(|| "user");

    let eval = use_hook(|| document::eval(SESSION_JS));

    use_future(move || async move {
        let mut eval = eval;
        loop {
            match eval.recv::<BridgeEvent>().await {
                Ok(event) => apply_bridge_event(
                    event,
                    error,
                    status,
                    partner,
                    you,
                    local_level,
                    remote_level,
                    kbps,
                    fps,
                    chat_log,
                ),
                Err(_) => break,
            }
        }
    });

    rsx! {
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        div { class: "app",
            match screen() {
                Screen::Lobby => rsx! {
                    Lobby {
                        name,
                        role,
                        instrument,
                        server_host,
                        server_port,
                        error,
                        on_join: move |_| {
                            spawn(async move {
                                start_lesson(
                                    eval,
                                    name,
                                    role,
                                    instrument,
                                    server_host,
                                    server_port,
                                    screen,
                                    error,
                                    status,
                                    partner,
                                    you,
                                    muted,
                                    camera_on,
                                    chat_log,
                                    facing,
                                )
                                .await;
                            });
                        },
                    }
                },
                Screen::Lesson => rsx! {
                    Lesson {
                        status,
                        error,
                        you,
                        partner,
                        muted,
                        camera_on,
                        local_level,
                        remote_level,
                        kbps,
                        fps,
                        chat_input,
                        chat_log,
                        on_leave: move |_| {
                            let _ = eval.send(serde_json::json!({ "op": "disconnect" }));
                            partner.set(None);
                            you.set(None);
                            muted.set(false);
                            camera_on.set(true);
                            chat_log.set(Vec::new());
                            status.set("Disconnected.".into());
                            screen.set(Screen::Lobby);
                        },
                        on_mute: move |_| {
                            let next = !muted();
                            muted.set(next);
                            let _ = eval.send(serde_json::json!({ "op": "set_muted", "muted": next }));
                            spawn(async move {
                                let _ = document::eval(&format!(
                                    "if (window.__lessonSetMuted) {{ await window.__lessonSetMuted({next}); }}"
                                ))
                                .await;
                            });
                        },
                        on_camera: move |_| {
                            let next = !camera_on();
                            camera_on.set(next);
                            let _ = eval.send(serde_json::json!({ "op": "set_camera", "enabled": next }));
                            spawn(async move {
                                let _ = document::eval(&format!(
                                    "if (window.__lessonSetCamera) {{ await window.__lessonSetCamera({next}); }}"
                                ))
                                .await;
                            });
                        },
                        on_flip: move |_| {
                            spawn(async move {
                                let next = if facing() == "user" { "environment" } else { "user" };
                                if request_media(next).await {
                                    facing.set(next);
                                    let _ = eval.send(serde_json::json!({ "op": "use_stream" }));
                                } else {
                                    error.set("Could not switch cameras.".into());
                                }
                            });
                        },
                        on_chat: move |_| {
                            let text = chat_input().trim().to_string();
                            if text.is_empty() {
                                return;
                            }
                            chat_log.write().push(("You".into(), text.clone()));
                            chat_input.set(String::new());
                            let _ = eval.send(serde_json::json!({ "op": "chat", "text": text }));
                        },
                    }
                },
            }
        }
    }
}

fn apply_bridge_event(
    event: BridgeEvent,
    mut error: Signal<String>,
    mut status: Signal<String>,
    mut partner: Signal<Option<PeerInfo>>,
    mut you: Signal<Option<PeerInfo>>,
    mut local_level: Signal<f32>,
    mut remote_level: Signal<f32>,
    mut kbps: Signal<u32>,
    mut fps: Signal<u32>,
    mut chat_log: Signal<Vec<(String, String)>>,
) {
    if event.event == "error" || event.msg_type == "error" {
        error.set(event.message.clone());
        status.set(event.message);
        return;
    }
    if event.event == "status" {
        status.set(event.message);
        return;
    }
    if event.event == "levels" {
        local_level.set(event.local);
        if event.remote > 0.0 {
            remote_level.set(event.remote);
        }
        return;
    }
    if event.event == "stats" {
        kbps.set(event.kbps);
        fps.set(event.fps);
        return;
    }

    match event.msg_type.as_str() {
        "welcome" => {
            you.set(event.you.clone());
            partner.set(event.partner.clone());
            status.set(if event.partner.is_some() {
                "Partner is in the studio.".into()
            } else {
                "Waiting for the other person to join...".into()
            });
        }
        "partner_joined" => {
            partner.set(event.partner);
            status.set("Partner is in the studio.".into());
        }
        "partner_left" => {
            partner.set(None);
            status.set("Partner left. Waiting for them to rejoin...".into());
        }
        "chat" => {
            if !event.text.is_empty() {
                chat_log.write().push((event.from, event.text));
            }
        }
        _ => {}
    }
}

async fn start_lesson(
    eval: document::Eval,
    name: Signal<String>,
    role: Signal<Role>,
    instrument: Signal<Instrument>,
    server_host: Signal<String>,
    server_port: Signal<String>,
    mut screen: Signal<Screen>,
    mut error: Signal<String>,
    mut status: Signal<String>,
    mut partner: Signal<Option<PeerInfo>>,
    mut you: Signal<Option<PeerInfo>>,
    mut muted: Signal<bool>,
    mut camera_on: Signal<bool>,
    mut chat_log: Signal<Vec<(String, String)>>,
    mut facing: Signal<&'static str>,
) {
    error.set(String::new());
    let name = name().trim().to_string();
    if name.is_empty() {
        error.set("Enter your name.".into());
        return;
    }
    let url = match build_server_url(&server_host(), &server_port()) {
        Ok(url) => url,
        Err(message) => {
            error.set(message);
            return;
        }
    };
    let chosen_facing = facing_for(role());
    if !request_media(chosen_facing).await {
        error.set("Allow camera and microphone so the other person can see and hear you.".into());
        return;
    }

    facing.set(chosen_facing);
    partner.set(None);
    you.set(None);
    muted.set(false);
    camera_on.set(true);
    chat_log.set(Vec::new());
    status.set("Connecting...".into());
    screen.set(Screen::Lesson);

    let _ = eval.send(ConnectCmd {
        op: "connect",
        url,
        join: ClientMessage::Join {
            name,
            role: role(),
            instrument: instrument(),
        },
        facing: chosen_facing,
    });
}

async fn request_media(facing: &str) -> bool {
    let script = format!(
        r#"
        window.__lessonFacing = "{facing}";
        try {{
            if (window.__lessonStream) {{
                window.__lessonStream.getTracks().forEach((track) => track.stop());
            }}
            window.__lessonStream = await navigator.mediaDevices.getUserMedia({{
                audio: {{
                    echoCancellation: false,
                    noiseSuppression: false,
                    autoGainControl: false,
                    channelCount: {{ ideal: 2 }},
                    sampleRate: {{ ideal: 48000 }},
                    latency: {{ ideal: 0.01 }}
                }},
                video: {{
                    width: {{ ideal: 1280 }},
                    height: {{ ideal: 720 }},
                    frameRate: {{ ideal: 30 }},
                    facingMode: "{facing}"
                }}
            }});
            return true;
        }} catch (err) {{
            return false;
        }}
        "#
    );
    matches!(
        document::eval(&script).await,
        Ok(serde_json::Value::Bool(true))
    )
}

#[component]
fn Lobby(
    name: Signal<String>,
    role: Signal<Role>,
    instrument: Signal<Instrument>,
    server_host: Signal<String>,
    server_port: Signal<String>,
    error: Signal<String>,
    on_join: EventHandler<()>,
) -> Element {
    rsx! {
        header { class: "hero",
            p { class: "kicker", "Piano & guitar" }
            h1 { "Lesson Studio" }
            p { class: "subtitle", "Two devices, one studio. Share camera and high-quality sound over a published server port." }
        }
        section { class: "card grid",
            label { class: "field",
                span { "Your name" }
                input {
                    placeholder: "Maya",
                    value: "{name}",
                    oninput: move |event| name.set(event.value()),
                }
            }
            div {
                p { class: "choice-label", "You are" }
                div { class: "choices",
                    button {
                        class: if role() == Role::Teacher { "active" } else { "" },
                        onclick: move |_| role.set(Role::Teacher),
                        "Teacher"
                    }
                    button {
                        class: if role() == Role::Student { "active" } else { "" },
                        onclick: move |_| role.set(Role::Student),
                        "Student"
                    }
                }
            }
            div {
                p { class: "choice-label", "Instrument" }
                div { class: "choices",
                    button {
                        class: if instrument() == Instrument::Piano { "active" } else { "" },
                        onclick: move |_| instrument.set(Instrument::Piano),
                        "Piano"
                    }
                    button {
                        class: if instrument() == Instrument::Guitar { "active" } else { "" },
                        onclick: move |_| instrument.set(Instrument::Guitar),
                        "Guitar"
                    }
                }
            }
            div { class: "row",
                label { class: "field",
                    span { "Server IP address" }
                    input {
                        placeholder: "192.168.1.10",
                        value: "{server_host}",
                        oninput: move |event| server_host.set(event.value()),
                    }
                }
                label { class: "field",
                    span { "Port" }
                    input {
                        class: "port",
                        value: "{server_port}",
                        oninput: move |event| server_port.set(event.value()),
                    }
                }
            }
            p { class: "hint",
                "Start lesson-server on Ubuntu or Windows, publish TCP port {DEFAULT_PORT}, then enter that machine's IP here. Headphones are strongly recommended so the instrument stays clean."
            }
            if !error().is_empty() {
                p { class: "error", "{error}" }
            }
            button { onclick: move |_| on_join.call(()), "Join studio" }
        }
    }
}

#[component]
fn Lesson(
    status: Signal<String>,
    error: Signal<String>,
    you: Signal<Option<PeerInfo>>,
    partner: Signal<Option<PeerInfo>>,
    muted: Signal<bool>,
    camera_on: Signal<bool>,
    local_level: Signal<f32>,
    remote_level: Signal<f32>,
    kbps: Signal<u32>,
    fps: Signal<u32>,
    chat_input: Signal<String>,
    chat_log: Signal<Vec<(String, String)>>,
    on_leave: EventHandler<()>,
    on_mute: EventHandler<()>,
    on_camera: EventHandler<()>,
    on_flip: EventHandler<()>,
    on_chat: EventHandler<()>,
) -> Element {
    let partner_label = partner()
        .map(|peer| {
            format!(
                "{} · {} · {}",
                peer.name,
                peer.role.label(),
                peer.instrument.label()
            )
        })
        .unwrap_or_else(|| "Waiting for partner".into());
    let you_label = you()
        .map(|peer| format!("You · {} · {}", peer.role.label(), peer.instrument.label()))
        .unwrap_or_else(|| "You".into());
    let local_pct = (local_level() * 100.0).clamp(0.0, 100.0);
    let remote_pct = (remote_level() * 100.0).clamp(0.0, 100.0);

    rsx! {
        div { class: "stage",
            video {
                id: "remote-video",
                class: "remote-video",
                autoplay: true,
                playsinline: true,
            }
            div { id: "remote-placeholder", class: "placeholder",
                div {
                    p { "{status}" }
                    p { class: "hint", "Point the camera at the keyboard or fretboard. Keep headphones on for studio-quality audio." }
                }
            }
            div { class: "badge", "{partner_label}" }
            div { id: "local-pip", class: "local-pip",
                video {
                    id: "local-preview",
                    class: "local-preview",
                    autoplay: true,
                    muted: true,
                    playsinline: true,
                }
                div { class: "badge", "{you_label}" }
            }
        }
        div { class: "status-bar",
            span { "{status} · {fps} fps · {kbps} kb/s" }
            div { class: "meters",
                div { class: "meter",
                    span { "You" }
                    div { class: "track",
                        div { class: "fill", style: "width: {local_pct}%" }
                    }
                }
                div { class: "meter",
                    span { "Partner" }
                    div { class: "track",
                        div { class: "fill", style: "width: {remote_pct}%" }
                    }
                }
            }
        }
        if !error().is_empty() {
            p { class: "error", "{error}" }
        }
        div { class: "toolbar",
            button { class: "secondary", onclick: move |_| on_mute.call(()),
                if muted() { "Unmute" } else { "Mute" }
            }
            button { class: "secondary", onclick: move |_| on_camera.call(()),
                if camera_on() { "Camera off" } else { "Camera on" }
            }
            button { class: "secondary", onclick: move |_| on_flip.call(()), "Flip camera" }
            button { class: "secondary", onclick: move |_| on_leave.call(()), "Leave" }
        }
        section { class: "card chat",
            div { class: "chat-log",
                for (from, text) in chat_log() {
                    p { strong { "{from}: " } "{text}" }
                }
            }
            div { class: "row",
                input {
                    placeholder: "Bar 12 again, slower",
                    value: "{chat_input}",
                    oninput: move |event| chat_input.set(event.value()),
                    onkeydown: move |event| {
                        if event.key() == Key::Enter {
                            on_chat.call(());
                        }
                    },
                }
                button { onclick: move |_| on_chat.call(()), "Send" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_ws_url_from_ip_and_default_port() {
        assert_eq!(
            build_server_url("192.168.1.10", "44041").unwrap(),
            "wss://192.168.1.10:44041/ws"
        );
    }

    #[test]
    fn accepts_full_websocket_url() {
        assert_eq!(
            build_server_url("ws://10.0.0.2:44041", "1").unwrap(),
            "wss://10.0.0.2:44041/ws"
        );
    }
}
