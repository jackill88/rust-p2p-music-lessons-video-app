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
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use lesson_protocol::{ClientMessage, DEFAULT_PORT};
use std::sync::Arc;
use studio::{Outbound, Studio};
use tokio::sync::mpsc;
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

    let write_task = tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            let ws_message = match message {
                Outbound::Text(text) => Message::Text(text.into()),
                Outbound::Binary(bytes) => Message::Binary(bytes),
            };
            if sender.send(ws_message).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = receiver.next().await {
        match message {
            Message::Text(text) => match serde_json::from_str::<ClientMessage>(&text) {
                Ok(ClientMessage::Join {
                    name,
                    role,
                    instrument,
                }) => {
                    tracing::info!(%name, ?role, ?instrument, "client joined the studio");
                    studio
                        .join(
                            &mut session,
                            lesson_protocol::PeerInfo {
                                name,
                                role,
                                instrument,
                            },
                        )
                        .await;
                }
                Ok(ClientMessage::Chat { text }) => {
                    studio.chat(&session, text).await;
                }
                Err(err) => {
                    tracing::warn!(error = %err, payload = %text, "invalid client message");
                    studio.send_error(&session, format!("Invalid message: {err}"));
                }
            },
            Message::Binary(payload) => {
                studio.forward_binary(&session, Bytes::from(payload)).await;
            }
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }

    studio.disconnect(&mut session).await;
    let _ = write_task.await;
}

async fn ws_handler(ws: WebSocketUpgrade, State(studio): State<Arc<Studio>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, studio))
}
