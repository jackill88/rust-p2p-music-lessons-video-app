use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 44041;
pub const MAX_PEERS: usize = 2;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Teacher,
    Student,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Self::Teacher => "Teacher",
            Self::Student => "Student",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Instrument {
    Piano,
    Guitar,
}

impl Instrument {
    pub fn label(self) -> &'static str {
        match self {
            Self::Piano => "Piano",
            Self::Guitar => "Guitar",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerInfo {
    pub name: String,
    pub role: Role,
    pub instrument: Instrument,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Join {
        name: String,
        role: Role,
        instrument: Instrument,
    },
    Chat {
        text: String,
    },
    Leave,
    Signal {
        kind: String,
        #[serde(default)]
        sdp: Option<String>,
        #[serde(default)]
        candidate: Option<serde_json::Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome {
        you: PeerInfo,
        partner: Option<PeerInfo>,
    },
    PartnerJoined {
        partner: PeerInfo,
    },
    PartnerLeft,
    Chat {
        from: String,
        text: String,
    },
    Signal {
        kind: String,
        #[serde(default)]
        sdp: Option<String>,
        #[serde(default)]
        candidate: Option<serde_json::Value>,
    },
    Error {
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_json_matches_js_shape() {
        let json = serde_json::to_string(&ClientMessage::Join {
            name: "Ada".into(),
            role: Role::Teacher,
            instrument: Instrument::Piano,
        })
        .unwrap();
        assert!(json.contains(r#""type":"join""#));
        assert!(json.contains(r#""role":"teacher""#));
        assert!(json.contains(r#""instrument":"piano""#));
    }
}
