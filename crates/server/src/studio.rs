use lesson_protocol::{PeerInfo, ServerMessage, MAX_PEERS};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};

/// Allow both people to reconnect while a dropped socket is still sitting in the list.
const MAX_CONNECTIONS: usize = MAX_PEERS + 2;

const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub enum Outbound {
    Text(String),
    Ping,
}

pub enum JoinOutcome {
    Joined,
    StudioFull,
}

struct Peer {
    info: Option<PeerInfo>,
    tx: mpsc::UnboundedSender<Outbound>,
    shutdown: Option<oneshot::Sender<()>>,
}

#[derive(Default)]
struct Inner {
    next_id: u32,
    peers: Vec<(u32, Peer)>,
}

pub struct Studio {
    inner: Mutex<Inner>,
    ping_interval: Duration,
    idle_timeout: Duration,
}

pub struct Session {
    id: u32,
    pub(crate) shutdown_rx: oneshot::Receiver<()>,
}

impl Studio {
    pub fn new() -> Self {
        Self::with_heartbeat(DEFAULT_PING_INTERVAL, DEFAULT_IDLE_TIMEOUT)
    }

    pub fn with_heartbeat(ping_interval: Duration, idle_timeout: Duration) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            ping_interval,
            idle_timeout,
        }
    }

    pub fn ping_interval(&self) -> Duration {
        self.ping_interval
    }

    pub fn idle_timeout(&self) -> Duration {
        self.idle_timeout
    }

    pub async fn register(&self, tx: mpsc::UnboundedSender<Outbound>) -> Option<Session> {
        let mut inner = self.inner.lock().await;
        inner.evict_unjoined_until(MAX_CONNECTIONS.saturating_sub(1));
        if inner.peers.len() >= MAX_CONNECTIONS {
            return None;
        }
        let id = inner.next_id;
        inner.next_id += 1;
        let (shutdown, shutdown_rx) = oneshot::channel();
        inner.peers.push((
            id,
            Peer {
                info: None,
                tx,
                shutdown: Some(shutdown),
            },
        ));
        Some(Session { id, shutdown_rx })
    }

    pub async fn join(&self, session: &mut Session, info: PeerInfo) -> JoinOutcome {
        let mut inner = self.inner.lock().await;
        if inner.peer(session.id).is_none() {
            return JoinOutcome::StudioFull;
        }

        let duplicates: Vec<u32> = inner
            .peers
            .iter()
            .filter_map(|(id, peer)| {
                if *id == session.id {
                    return None;
                }
                let existing = peer.info.as_ref()?;
                if same_person(&existing.name, &info.name) {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();

        for id in duplicates {
            tracing::info!(name = %info.name, "replacing stale studio seat");
            if let Some(peer) = inner.take_peer(id) {
                let _ = peer.shutdown.and_then(|tx| tx.send(()).ok());
            }
        }

        let occupied = inner
            .peers
            .iter()
            .filter(|(id, peer)| *id != session.id && peer.info.is_some())
            .count();
        if occupied >= MAX_PEERS {
            return JoinOutcome::StudioFull;
        }

        let Some(peer) = inner.peer_mut(session.id) else {
            return JoinOutcome::StudioFull;
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

        JoinOutcome::Joined
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

    pub async fn send_error(&self, session: &Session, message: String) {
        let inner = self.inner.lock().await;
        if let Some(peer) = inner.peer(session.id) {
            send_json(&peer.tx, &ServerMessage::Error { message });
        }
    }

    pub async fn disconnect(&self, session: &mut Session) {
        let mut inner = self.inner.lock().await;
        let Some(peer) = inner.take_peer(session.id) else {
            return;
        };
        let was_joined = peer.info.is_some();
        if was_joined {
            if let Some(partner) = inner.partner(session.id) {
                send_json(&partner.tx, &ServerMessage::PartnerLeft);
            }
        }
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

    fn take_peer(&mut self, id: u32) -> Option<Peer> {
        let idx = self.peers.iter().position(|(peer_id, _)| *peer_id == id)?;
        Some(self.peers.remove(idx).1)
    }

    fn evict_unjoined_until(&mut self, max_keep: usize) {
        while self.peers.len() > max_keep {
            let Some(idx) = self.peers.iter().position(|(_, peer)| peer.info.is_none()) else {
                break;
            };
            let peer = self.peers.remove(idx).1;
            let _ = peer.shutdown.and_then(|tx| tx.send(()).ok());
        }
    }

    fn partner(&self, id: u32) -> Option<&Peer> {
        self.peers
            .iter()
            .find(|(peer_id, peer)| *peer_id != id && peer.info.is_some())
            .map(|(_, peer)| peer)
    }

    fn partner_mut(&mut self, id: u32) -> Option<&mut Peer> {
        self.peers
            .iter_mut()
            .find(|(peer_id, peer)| *peer_id != id && peer.info.is_some())
            .map(|(_, peer)| peer)
    }

    fn partner_info(&self, id: u32) -> Option<PeerInfo> {
        self.partner(id).and_then(|peer| peer.info.clone())
    }
}

fn same_person(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn send_json(tx: &mpsc::UnboundedSender<Outbound>, message: &ServerMessage) {
    if let Ok(text) = serde_json::to_string(message) {
        let _ = tx.send(Outbound::Text(text));
    }
}
