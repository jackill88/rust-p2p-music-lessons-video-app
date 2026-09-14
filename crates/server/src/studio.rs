use bytes::Bytes;
use lesson_protocol::{PeerInfo, ServerMessage, MAX_PEERS};
use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub enum Outbound {
    Text(String),
    Binary(Bytes),
}

struct Peer {
    info: Option<PeerInfo>,
    tx: mpsc::UnboundedSender<Outbound>,
}

#[derive(Default)]
struct Inner {
    next_id: u32,
    peers: Vec<(u32, Peer)>,
}

pub struct Studio {
    inner: Mutex<Inner>,
}

pub struct Session {
    id: u32,
}

impl Studio {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }

    pub async fn register(&self, tx: mpsc::UnboundedSender<Outbound>) -> Option<Session> {
        let mut inner = self.inner.lock().await;
        if inner.peers.len() >= MAX_PEERS {
            return None;
        }
        let id = inner.next_id;
        inner.next_id += 1;
        inner.peers.push((id, Peer { info: None, tx }));
        Some(Session { id })
    }

    pub async fn join(&self, session: &mut Session, info: PeerInfo) {
        let mut inner = self.inner.lock().await;
        let Some(peer) = inner.peer_mut(session.id) else {
            return;
        };
        peer.info = Some(info.clone());

        let partner = inner.partner_info(session.id);
        send_json(
            &inner.peer(session.id).unwrap().tx,
            &ServerMessage::Welcome {
                you: info.clone(),
                partner: partner.clone(),
            },
        );

        if partner.is_some() {
            if let Some(partner_peer) = inner.partner_mut(session.id) {
                send_json(
                    &partner_peer.tx,
                    &ServerMessage::PartnerJoined { partner: info },
                );
            }
        }
    }

    pub async fn signal(
        &self,
        session: &Session,
        kind: String,
        sdp: Option<String>,
        candidate: Option<serde_json::Value>,
    ) {
        let inner = self.inner.lock().await;
        if let Some(partner) = inner.partner(session.id) {
            send_json(
                &partner.tx,
                &ServerMessage::Signal {
                    kind,
                    sdp,
                    candidate,
                },
            );
        }
    }

    pub async fn chat(&self, session: &Session, text: String) {
        let inner = self.inner.lock().await;
        let Some(from) = inner
            .peer(session.id)
            .and_then(|peer| peer.info.as_ref().map(|info| info.name.clone()))
        else {
            return;
        };
        if let Some(partner) = inner.partner(session.id) {
            send_json(&partner.tx, &ServerMessage::Chat { from, text });
        }
    }

    pub async fn forward_binary(&self, session: &Session, payload: Bytes) {
        let inner = self.inner.lock().await;
        if let Some(partner) = inner.partner(session.id) {
            let _ = partner.tx.send(Outbound::Binary(payload));
        }
    }

    pub fn send_error(&self, session: &Session, message: String) {
        // Best-effort; the session may already be gone.
        if let Ok(inner) = self.inner.try_lock() {
            if let Some(peer) = inner.peer(session.id) {
                send_json(&peer.tx, &ServerMessage::Error { message });
            }
        }
    }

    pub async fn disconnect(&self, session: &mut Session) {
        let mut inner = self.inner.lock().await;
        if let Some(partner) = inner.partner(session.id) {
            send_json(&partner.tx, &ServerMessage::PartnerLeft);
        }
        inner.peers.retain(|(id, _)| *id != session.id);
    }
}

impl Inner {
    fn peer(&self, id: u32) -> Option<&Peer> {
        self.peers
            .iter()
            .find(|(peer_id, _)| *peer_id == id)
            .map(|(_, peer)| peer)
    }

    fn peer_mut(&mut self, id: u32) -> Option<&mut Peer> {
        self.peers
            .iter_mut()
            .find(|(peer_id, _)| *peer_id == id)
            .map(|(_, peer)| peer)
    }

    fn partner(&self, id: u32) -> Option<&Peer> {
        self.peers
            .iter()
            .find(|(peer_id, _)| *peer_id != id)
            .map(|(_, peer)| peer)
    }

    fn partner_mut(&mut self, id: u32) -> Option<&mut Peer> {
        self.peers
            .iter_mut()
            .find(|(peer_id, _)| *peer_id != id)
            .map(|(_, peer)| peer)
    }

    fn partner_info(&self, id: u32) -> Option<PeerInfo> {
        self.partner(id).and_then(|peer| peer.info.clone())
    }
}

fn send_json(tx: &mpsc::UnboundedSender<Outbound>, message: &ServerMessage) {
    if let Ok(text) = serde_json::to_string(message) {
        let _ = tx.send(Outbound::Text(text));
    }
}
