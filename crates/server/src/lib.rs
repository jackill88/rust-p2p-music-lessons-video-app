mod studio;
mod tls;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use lesson_protocol::{ClientMessage, DEFAULT_PORT};
use std::sync::Arc;
use studio::{JoinOutcome, Outbound, Studio};
use tokio::sync::mpsc;
use tokio::time::{interval_at, timeout, Duration, Instant, MissedTickBehavior};
use tower_http::cors::CorsLayer;

pub use studio::Studio as LessonStudio;
pub use tls::self_signed_pem;
pub const LISTEN_PORT: u16 = DEFAULT_PORT;

pub fn app(studio: Arc<Studio>) -> Router {
    Router::new()
        .route("/", get(index_page))
        .route("/health", get(|| async { "ok" }))
        .route("/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(studio)
}

async fn index_page() -> Html<String> {
    Html(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Lesson Studio</title>
  <style>
    body {{
      margin: 0;
      min-height: 100vh;
      font-family: "Segoe UI", system-ui, sans-serif;
      background: #1a1410;
      color: #f4e6d4;
      display: grid;
      place-items: center;
    }}
    main {{
      max-width: 36rem;
      padding: 2rem;
      border: 1px solid #3d2e22;
      border-radius: 1.25rem;
      background: #241c16;
    }}
    h1 {{ margin: 0 0 0.5rem; }}
    p {{ color: #cbb79a; line-height: 1.5; }}
    code {{ color: #e8c07a; }}
  </style>
</head>
<body>
  <main>
    <h1>Lesson Studio</h1>
    <p>This host is the lesson server. Publish TCP port <code>{LISTEN_PORT}</code>, then open the Lesson Studio app on a PC or Android device and enter this machine's IP address.</p>
    <p>Camera and microphone run inside the app, not in this browser page.</p>
  </main>
</body>
</html>"#
    ))
}

pub async fn handle_socket(socket: WebSocket, studio: Arc<Studio>) {
    let (mut sender, mut receiver) = socket.split();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Outbound>();
    let ping_tx = outbound_tx.clone();

    let Some(mut session) = studio.register(outbound_tx).await else {
        tracing::warn!("studio is full; rejecting extra client");
        let payload = serde_json::json!({
            "type": "error",
            "message": "This studio already has two people. Wait for a seat or start the server on another host."
        })
        .to_string();
        let _ = sender.send(Message::Text(payload.into())).await;
        let _ = sender.send(Message::Close(None)).await;
        return;
    };

    let mut write_task = tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            let ws_message = match message {
                Outbound::Text(text) => Message::Text(text.into()),
                Outbound::Ping => Message::Ping(vec![].into()),
            };
            if sender.send(ws_message).await.is_err() {
                break;
            }
        }
    });

    let mut last_seen = Instant::now();
    let mut ping_ticks = interval_at(
        Instant::now() + studio.ping_interval(),
        studio.ping_interval(),
    );
    ping_ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = &mut write_task => break,
            _ = &mut session.shutdown_rx => {
                tracing::info!("session replaced by a reconnect");
                break;
            }
            _ = ping_ticks.tick() => {
                if last_seen.elapsed() >= studio.idle_timeout() {
                    tracing::info!("dropping idle studio connection");
                    break;
                }
                let _ = ping_tx.send(Outbound::Ping);
            }
            message = receiver.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    last_seen = Instant::now();
                    match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(ClientMessage::Join {
                            name,
                            role,
                            instrument,
                        }) => {
                            tracing::info!(%name, ?role, ?instrument, "client joined the studio");
                            match studio
                                .join(
                                    &mut session,
                                    lesson_protocol::PeerInfo {
                                        name,
                                        role,
                                        instrument,
                                    },
                                )
                                .await
                            {
                                JoinOutcome::Joined => {}
                                JoinOutcome::StudioFull => {
                                    studio.send_error(
                                        &session,
                                        "This studio already has two people. Wait for a seat or start the server on another host.".into(),
                                    )
                                    .await;
                                    break;
                                }
                            }
                        }
                        Ok(ClientMessage::Chat { text }) => {
                            studio.chat(&session, text).await;
                        }
                        Ok(ClientMessage::Leave) => break,
                        Ok(ClientMessage::Signal {
                            kind,
                            sdp,
                            candidate,
                        }) => {
                            studio.signal(&session, kind, sdp, candidate).await;
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, payload = %text, "invalid client message");
                            studio.send_error(&session, format!("Invalid message: {err}")).await;
                        }
                    }
                }
                Some(Ok(Message::Pong(_))) | Some(Ok(Message::Ping(_))) => {
                    last_seen = Instant::now();
                }
                Some(Ok(Message::Binary(_))) => {}
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
            },
        }
    }

    drop(ping_tx);
    studio.disconnect(&mut session).await;
    if timeout(Duration::from_millis(200), &mut write_task)
        .await
        .is_err()
    {
        write_task.abort();
        let _ = write_task.await;
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(studio): State<Arc<Studio>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, studio))
}
