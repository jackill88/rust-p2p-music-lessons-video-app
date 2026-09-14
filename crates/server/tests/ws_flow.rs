use futures_util::{SinkExt, StreamExt};
use lesson_protocol::{ClientMessage, Instrument, Role, ServerMessage};
use lesson_server::{app, LessonStudio};
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

async fn recv_server_message(
    read: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> ServerMessage {
    loop {
        match read.next().await {
            Some(Ok(Message::Text(text))) => {
                if let Ok(message) = serde_json::from_str::<ServerMessage>(&text) {
                    return message;
                }
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            other => panic!("websocket closed before server message: {other:?}"),
        }
    }
}

async fn spawn_server() -> String {
    let studio = Arc::new(LessonStudio::new());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app(studio)).await.unwrap();
    });
    format!("ws://{addr}/ws")
}

fn join_payload(name: &str, role: Role, instrument: Instrument) -> String {
    serde_json::to_string(&ClientMessage::Join {
        name: name.into(),
        role,
        instrument,
    })
    .unwrap()
}

#[tokio::test]
async fn two_clients_pair_and_relay_signals() {
    let url = spawn_server().await;

    let (teacher_ws, _) = connect_async(&url).await.unwrap();
    let (mut teacher_write, mut teacher_read) = teacher_ws.split();
    teacher_write
        .send(Message::Text(
            join_payload("Maya", Role::Teacher, Instrument::Piano).into(),
        ))
        .await
        .unwrap();

    match recv_server_message(&mut teacher_read).await {
        ServerMessage::Welcome { partner, you } => {
            assert_eq!(you.name, "Maya");
            assert!(partner.is_none());
        }
        other => panic!("unexpected first teacher message: {other:?}"),
    }

    let (student_ws, _) = connect_async(&url).await.unwrap();
    let (mut student_write, mut student_read) = student_ws.split();
    student_write
        .send(Message::Text(
            join_payload("Leo", Role::Student, Instrument::Guitar).into(),
        ))
        .await
        .unwrap();

    match recv_server_message(&mut student_read).await {
        ServerMessage::Welcome { partner, you } => {
            assert_eq!(you.name, "Leo");
            assert_eq!(partner.unwrap().name, "Maya");
        }
        other => panic!("unexpected student welcome: {other:?}"),
    }

    match recv_server_message(&mut teacher_read).await {
        ServerMessage::PartnerJoined { partner } => {
            assert_eq!(partner.name, "Leo");
            assert_eq!(partner.instrument, Instrument::Guitar);
        }
        other => panic!("teacher should learn the student joined: {other:?}"),
    }

    let signal = serde_json::to_string(&ClientMessage::Signal {
        kind: "offer".into(),
        sdp: Some("v=0".into()),
        candidate: None,
    })
    .unwrap();
    teacher_write
        .send(Message::Text(signal.into()))
        .await
        .unwrap();
    match recv_server_message(&mut student_read).await {
        ServerMessage::Signal { kind, sdp, .. } => {
            assert_eq!(kind, "offer");
            assert_eq!(sdp.as_deref(), Some("v=0"));
        }
        other => panic!("student should receive WebRTC signal: {other:?}"),
    }
}

#[tokio::test]
async fn third_client_is_rejected() {
    let url = spawn_server().await;

    let (first_ws, _) = connect_async(&url).await.unwrap();
    let (mut first_write, mut first_read) = first_ws.split();
    first_write
        .send(Message::Text(
            join_payload("Maya", Role::Teacher, Instrument::Piano).into(),
        ))
        .await
        .unwrap();
    assert!(matches!(
        recv_server_message(&mut first_read).await,
        ServerMessage::Welcome { .. }
    ));

    let (second_ws, _) = connect_async(&url).await.unwrap();
    let (mut second_write, mut second_read) = second_ws.split();
    second_write
        .send(Message::Text(
            join_payload("Leo", Role::Student, Instrument::Guitar).into(),
        ))
        .await
        .unwrap();
    assert!(matches!(
        recv_server_message(&mut second_read).await,
        ServerMessage::Welcome { .. }
    ));
    assert!(matches!(
        recv_server_message(&mut first_read).await,
        ServerMessage::PartnerJoined { .. }
    ));

    let (third_ws, _) = connect_async(&url).await.unwrap();
    let (mut third_write, mut third_read) = third_ws.split();
    third_write
        .send(Message::Text(
            join_payload("Ada", Role::Student, Instrument::Piano).into(),
        ))
        .await
        .unwrap();

    match recv_server_message(&mut third_read).await {
        ServerMessage::Error { message } => {
            assert!(message.contains("two people"), "{message}");
        }
        other => panic!("third client should be rejected: {other:?}"),
    }
}

#[tokio::test]
async fn same_name_reclaims_a_stale_seat() {
    let url = spawn_server().await;

    let (teacher_ws, _) = connect_async(&url).await.unwrap();
    let (mut teacher_write, mut teacher_read) = teacher_ws.split();
    teacher_write
        .send(Message::Text(
            join_payload("Maya", Role::Teacher, Instrument::Piano).into(),
        ))
        .await
        .unwrap();
    assert!(matches!(
        recv_server_message(&mut teacher_read).await,
        ServerMessage::Welcome { partner: None, .. }
    ));

    let (student_ws, _) = connect_async(&url).await.unwrap();
    let (mut student_write, mut student_read) = student_ws.split();
    student_write
        .send(Message::Text(
            join_payload("Leo", Role::Student, Instrument::Guitar).into(),
        ))
        .await
        .unwrap();
    assert_eq!(
        recv_server_message(&mut student_read).await,
        ServerMessage::Welcome {
            you: lesson_protocol::PeerInfo {
                name: "Leo".into(),
                role: Role::Student,
                instrument: Instrument::Guitar,
            },
            partner: Some(lesson_protocol::PeerInfo {
                name: "Maya".into(),
                role: Role::Teacher,
                instrument: Instrument::Piano,
            }),
        }
    );
    assert!(matches!(
        recv_server_message(&mut teacher_read).await,
        ServerMessage::PartnerJoined { .. }
    ));

    let (teacher_again_ws, _) = connect_async(&url).await.unwrap();
    let (mut teacher_again_write, mut teacher_again_read) = teacher_again_ws.split();
    teacher_again_write
        .send(Message::Text(
            join_payload("maya", Role::Teacher, Instrument::Piano).into(),
        ))
        .await
        .unwrap();

    match recv_server_message(&mut teacher_again_read).await {
        ServerMessage::Welcome { partner, you } => {
            assert_eq!(you.name, "maya");
            assert_eq!(partner.unwrap().name, "Leo");
        }
        other => panic!("reconnecting teacher should be welcomed: {other:?}"),
    }

    match recv_server_message(&mut student_read).await {
        ServerMessage::PartnerJoined { partner } => {
            assert_eq!(partner.name, "maya");
        }
        other => panic!("student should see the teacher return: {other:?}"),
    }
}

#[tokio::test]
async fn leave_message_frees_the_seat() {
    let url = spawn_server().await;

    let (teacher_ws, _) = connect_async(&url).await.unwrap();
    let (mut teacher_write, mut teacher_read) = teacher_ws.split();
    teacher_write
        .send(Message::Text(
            join_payload("Maya", Role::Teacher, Instrument::Piano).into(),
        ))
        .await
        .unwrap();
    assert!(matches!(
        recv_server_message(&mut teacher_read).await,
        ServerMessage::Welcome { .. }
    ));

    let (student_ws, _) = connect_async(&url).await.unwrap();
    let (mut student_write, mut student_read) = student_ws.split();
    student_write
        .send(Message::Text(
            join_payload("Leo", Role::Student, Instrument::Guitar).into(),
        ))
        .await
        .unwrap();
    assert!(matches!(
        recv_server_message(&mut student_read).await,
        ServerMessage::Welcome { .. }
    ));
    assert!(matches!(
        recv_server_message(&mut teacher_read).await,
        ServerMessage::PartnerJoined { .. }
    ));

    teacher_write
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::Leave).unwrap().into(),
        ))
        .await
        .unwrap();
    match recv_server_message(&mut student_read).await {
        ServerMessage::PartnerLeft => {}
        other => panic!("student should see the teacher leave: {other:?}"),
    }

    let (replacement_ws, _) = connect_async(&url).await.unwrap();
    let (mut replacement_write, mut replacement_read) = replacement_ws.split();
    replacement_write
        .send(Message::Text(
            join_payload("Ada", Role::Teacher, Instrument::Piano).into(),
        ))
        .await
        .unwrap();
    match recv_server_message(&mut replacement_read).await {
        ServerMessage::Welcome { partner, you } => {
            assert_eq!(you.name, "Ada");
            assert_eq!(partner.unwrap().name, "Leo");
        }
        other => panic!("leave should free a seat: {other:?}"),
    }
}

#[tokio::test]
async fn health_endpoint_is_ok() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let studio = Arc::new(LessonStudio::new());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app(studio)).await.unwrap();
    });

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).await.unwrap();
    let body = buf
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .trim()
        .to_string();
    assert_eq!(body, "ok");
}
