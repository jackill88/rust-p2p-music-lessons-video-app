use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 44041;
pub const MAX_PEERS: usize = 2;

pub const MEDIA_VIDEO_JPEG: u8 = 1;
pub const MEDIA_AUDIO_PCM: u8 = 2;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoPacket<'a> {
    pub timestamp_ms: u32,
    pub width: u16,
    pub height: u16,
    pub jpeg: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioPacket<'a> {
    pub timestamp_ms: u32,
    pub channels: u8,
    pub sample_rate: u32,
    pub pcm: &'a [u8],
}

pub fn encode_video(timestamp_ms: u32, width: u16, height: u16, jpeg: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(9 + jpeg.len());
    packet.push(MEDIA_VIDEO_JPEG);
    packet.extend_from_slice(&timestamp_ms.to_be_bytes());
    packet.extend_from_slice(&width.to_be_bytes());
    packet.extend_from_slice(&height.to_be_bytes());
    packet.extend_from_slice(jpeg);
    packet
}

pub fn decode_video(packet: &[u8]) -> Option<VideoPacket<'_>> {
    if packet.len() < 9 || packet[0] != MEDIA_VIDEO_JPEG {
        return None;
    }
    Some(VideoPacket {
        timestamp_ms: u32::from_be_bytes(packet[1..5].try_into().ok()?),
        width: u16::from_be_bytes(packet[5..7].try_into().ok()?),
        height: u16::from_be_bytes(packet[7..9].try_into().ok()?),
        jpeg: &packet[9..],
    })
}

pub fn encode_audio(timestamp_ms: u32, channels: u8, sample_rate: u32, pcm: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(10 + pcm.len());
    packet.push(MEDIA_AUDIO_PCM);
    packet.extend_from_slice(&timestamp_ms.to_be_bytes());
    packet.push(channels);
    packet.extend_from_slice(&sample_rate.to_be_bytes());
    packet.extend_from_slice(pcm);
    packet
}

pub fn decode_audio(packet: &[u8]) -> Option<AudioPacket<'_>> {
    if packet.len() < 10 || packet[0] != MEDIA_AUDIO_PCM {
        return None;
    }
    Some(AudioPacket {
        timestamp_ms: u32::from_be_bytes(packet[1..5].try_into().ok()?),
        channels: packet[5],
        sample_rate: u32::from_be_bytes(packet[6..10].try_into().ok()?),
        pcm: &packet[10..],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_roundtrip() {
        let encoded = encode_video(1_500, 1280, 720, b"jpeg-bytes");
        let decoded = decode_video(&encoded).unwrap();
        assert_eq!(decoded.timestamp_ms, 1_500);
        assert_eq!(decoded.width, 1280);
        assert_eq!(decoded.height, 720);
        assert_eq!(decoded.jpeg, b"jpeg-bytes");
    }

    #[test]
    fn audio_roundtrip() {
        let pcm = vec![0u8, 1, 2, 3];
        let encoded = encode_audio(40, 2, 48_000, &pcm);
        let decoded = decode_audio(&encoded).unwrap();
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.sample_rate, 48_000);
        assert_eq!(decoded.pcm, pcm);
    }

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
