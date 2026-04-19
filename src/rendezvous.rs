use crate::{CURRENT_PROTOCOL_VERSION, PairingIntroEnvelope};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinRequest {
    pub protocol_version: u16,
    pub slot: u16,
    pub peer_id: String,
    pub expires_at_unix_secs: u64,
}

impl JoinRequest {
    pub fn new(slot: u16, peer_id: impl Into<String>, expires_at_unix_secs: u64) -> Self {
        Self {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            slot,
            peer_id: peer_id.into(),
            expires_at_unix_secs,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RendezvousErrorCode {
    SessionExpired,
    SessionFull,
    InvalidPayload,
    InvalidState,
    VersionMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClientMessage {
    Join(JoinRequest),
    RelayPake {
        slot: u16,
        peer_id: String,
        payload: Vec<u8>,
    },
    RelayBootstrap {
        slot: u16,
        peer_id: String,
        envelope: PairingIntroEnvelope,
    },
    Complete {
        slot: u16,
        peer_id: String,
    },
    Cancel {
        slot: u16,
        peer_id: String,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerMessage {
    Joined {
        slot: u16,
        peer_id: String,
    },
    PeerJoined {
        slot: u16,
        peer_id: String,
    },
    RelayPake {
        slot: u16,
        from_peer_id: String,
        payload: Vec<u8>,
    },
    RelayBootstrap {
        slot: u16,
        from_peer_id: String,
        envelope: PairingIntroEnvelope,
    },
    Established {
        slot: u16,
        with_peer_id: String,
    },
    PeerLeft {
        slot: u16,
        peer_id: String,
    },
    Expired {
        slot: u16,
    },
    Error {
        slot: u16,
        code: RendezvousErrorCode,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{ClientMessage, JoinRequest, RendezvousErrorCode, ServerMessage};
    use crate::PairingIntroEnvelope;

    #[test]
    fn client_messages_round_trip_through_json() {
        let message = ClientMessage::RelayBootstrap {
            slot: 2048,
            peer_id: "peer-a".to_string(),
            envelope: PairingIntroEnvelope {
                nonce: [7u8; 24],
                ciphertext: vec![1, 2, 3, 4],
            },
        };

        let encoded = serde_json::to_string(&message).unwrap();
        let decoded: ClientMessage = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn server_messages_round_trip_through_json() {
        let message = ServerMessage::Error {
            slot: 2048,
            code: RendezvousErrorCode::SessionFull,
            message: "two peers already connected".to_string(),
        };

        let encoded = serde_json::to_string(&message).unwrap();
        let decoded: ServerMessage = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn join_request_sets_current_version() {
        let request = JoinRequest::new(2048, "peer-a", 1_700_000_100);
        assert_eq!(request.protocol_version, crate::CURRENT_PROTOCOL_VERSION);
    }
}
