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
    kbps: u32,
    #[serde(default)]
    fps: u32,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    remaining: u32,
    #[serde(default)]
    phase: String,
    #[serde(default)]
    gain: f32,
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
    let mut levels_on = use_signal(|| true);
    let kbps = use_signal(|| 0u32);
    let fps = use_signal(|| 0u32);
    let mut chat_input = use_signal(String::new);
    let mut chat_log = use_signal(Vec::<(String, String)>::new);
    let mut facing = use_signal(|| "user");
    let mut feedback = use_signal(String::new);
    let mut mic_gain = use_signal(|| 1.0f32);
    let mut calibrating = use_signal(|| false);
    let mut calibrate_remaining = use_signal(|| 0u32);
    let mut calibrate_note = use_signal(String::new);

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
                    kbps,
                    fps,
                    chat_log,
                    feedback,
                    mic_gain,
                    calibrating,
                    calibrate_remaining,
                    calibrate_note,
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
                                    mic_gain,
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
                        levels_on,
                        kbps,
                        fps,
                        chat_input,
                        chat_log,
                        feedback,
                        mic_gain,
                        calibrating,
                        calibrate_remaining,
                        calibrate_note,
                        on_leave: move |_| {
                            let _ = eval.send(serde_json::json!({ "op": "disconnect" }));
                            partner.set(None);
                            you.set(None);
                            muted.set(false);
                            camera_on.set(true);
                            levels_on.set(true);
                            mic_gain.set(1.0);
                            calibrating.set(false);
                            calibrate_remaining.set(0);
                            calibrate_note.set(String::new());
                            chat_log.set(Vec::new());
                            feedback.set(String::new());
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
                        on_levels: move |_| {
                            let next = !levels_on();
                            levels_on.set(next);
                            let _ = eval.send(serde_json::json!({ "op": "set_levels", "enabled": next }));
                            spawn(async move {
                                let _ = document::eval(&format!(
                                    "if (window.__lessonSetLevels) {{ window.__lessonSetLevels({next}); }}"
                                ))
                                .await;
                            });
                        },
                        on_gain: move |value: f32| {
                            let next = value.clamp(0.25, 4.0);
                            mic_gain.set(next);
                            let _ = eval.send(serde_json::json!({ "op": "set_gain", "value": next }));
                            spawn(async move {
                                let _ = document::eval(&format!(
                                    "if (window.__lessonSetGain) {{ window.__lessonSetGain({next}); }}"
                                ))
                                .await;
                            });
                        },
                        on_calibrate: move |_| {
                            if muted() {
                                calibrate_note.set("Unmute the microphone before calibrating.".into());
                                return;
                            }
                            calibrating.set(true);
                            calibrate_remaining.set(4);
                            calibrate_note.set("Now play loudly".into());
                            let _ = eval.send(serde_json::json!({ "op": "calibrate" }));
                            spawn(async move {
                                let _ = document::eval(
                                    "if (window.__lessonCalibrate) { window.__lessonCalibrate(); }",
                                )
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
    mut kbps: Signal<u32>,
    mut fps: Signal<u32>,
    mut chat_log: Signal<Vec<(String, String)>>,
    mut feedback: Signal<String>,
    mut mic_gain: Signal<f32>,
    mut calibrating: Signal<bool>,
    mut calibrate_remaining: Signal<u32>,
    mut calibrate_note: Signal<String>,
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
    if event.event == "feedback" {
        feedback.set(if event.active {
            event.message
        } else {
            String::new()
        });
        return;
    }
    if event.event == "stats" {
        kbps.set(event.kbps);
        fps.set(event.fps);
        return;
    }
    if event.event == "gain" && event.gain > 0.0 {
        mic_gain.set(event.gain.clamp(0.25, 4.0));
        return;
    }
    if event.event == "calibrate" {
        match event.phase.as_str() {
            "play" => {
                calibrating.set(true);
                calibrate_remaining.set(event.remaining);
                calibrate_note.set(event.message);
            }
            "done" => {
                calibrating.set(false);
                calibrate_remaining.set(0);
                if event.gain > 0.0 {
                    mic_gain.set(event.gain.clamp(0.25, 4.0));
                }
                calibrate_note.set(event.message);
            }
            "fail" => {
                calibrating.set(false);
                calibrate_remaining.set(0);
                calibrate_note.set(event.message);
            }
            _ => {
                calibrating.set(false);
                calibrate_remaining.set(0);
            }
        }
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
    mut mic_gain: Signal<f32>,
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
    mic_gain.set(1.0);
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
    levels_on: Signal<bool>,
    kbps: Signal<u32>,
    fps: Signal<u32>,
    chat_input: Signal<String>,
    chat_log: Signal<Vec<(String, String)>>,
    feedback: Signal<String>,
    mic_gain: Signal<f32>,
    calibrating: Signal<bool>,
    calibrate_remaining: Signal<u32>,
    calibrate_note: Signal<String>,
    on_leave: EventHandler<()>,
    on_mute: EventHandler<()>,
    on_camera: EventHandler<()>,
    on_levels: EventHandler<()>,
    on_gain: EventHandler<f32>,
    on_calibrate: EventHandler<()>,
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
    let gain_label = format!("{:.1}", mic_gain());
    let mut tray_open = use_signal(|| false);
    let tray_expanded = tray_open() || calibrating();

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
            div { class: "stage-hud",
                div { class: "badge", "{partner_label}" }
                div {
                    id: "level-meters",
                    class: if levels_on() { "stage-meters" } else { "stage-meters is-hidden" },
                    div { class: "meter",
                        span { "You" }
                        div { class: "track",
                            div { id: "local-level-fill", class: "fill" }
                        }
                    }
                    div { class: "meter",
                        span { "Partner" }
                        div { class: "track",
                            div { id: "remote-level-fill", class: "fill" }
                        }
                    }
                }
            }
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
            if calibrating() {
                div { class: "calibrate-banner",
                    p { "Now play loudly" }
                    p { class: "calibrate-count", "{calibrate_remaining}" }
                }
            }
        }
        div { class: "status-bar",
            span { "{status} · {fps} fps · {kbps} kb/s" }
        }
        if !error().is_empty() {
            p { class: "error", "{error}" }
        }
        if !feedback().is_empty() {
            p { class: "warning", "{feedback}" }
        }
        div { class: "toolbar",
            button { class: "secondary", onclick: move |_| on_mute.call(()),
                if muted() { "Unmute" } else { "Mute" }
            }
            button { class: "secondary", onclick: move |_| on_camera.call(()),
                if camera_on() { "Camera off" } else { "Camera on" }
            }
            button { class: "secondary", onclick: move |_| on_levels.call(()),
                if levels_on() { "Levels off" } else { "Levels on" }
            }
            button { class: "secondary", onclick: move |_| on_flip.call(()), "Flip camera" }
            button { class: "secondary", onclick: move |_| on_leave.call(()), "Leave" }
        }
        div { class: if tray_expanded { "tray is-open" } else { "tray" },
            button {
                class: "tray-handle",
                onclick: move |_| tray_open.set(!tray_open()),
                span { "Mic {gain_label}×" }
                span { class: "tray-chevron", if tray_expanded { "Minimize" } else { "Settings" } }
            }
            if tray_expanded {
                div { class: "tray-body",
                    label { class: "field",
                        span { "Sensitivity" }
                        input {
                            r#type: "range",
                            min: "0.25",
                            max: "4",
                            step: "0.05",
                            value: "{mic_gain}",
                            disabled: calibrating(),
                            oninput: move |event| {
                                if let Ok(value) = event.value().parse::<f32>() {
                                    on_gain.call(value);
                                }
                            },
                        }
                    }
                    button {
                        class: "secondary",
                        disabled: calibrating() || muted(),
                        onclick: move |_| on_calibrate.call(()),
                        if calibrating() {
                            "Now play loudly ({calibrate_remaining})"
                        } else {
                            "Play loudly to set level"
                        }
                    }
                    if !calibrate_note().is_empty() && !calibrating() {
                        p { class: "hint", "{calibrate_note}" }
                    }
                }
            }
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
