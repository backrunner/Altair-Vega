use iroh_tickets::endpoint::EndpointTicket;
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerCapabilities {
    pub messages: bool,
    pub files: bool,
    pub folder_sync: bool,
}

impl PeerCapabilities {
    pub const fn new(messages: bool, files: bool, folder_sync: bool) -> Self {
        Self {
            messages,
            files,
            folder_sync,
        }
    }

    pub const fn cli() -> Self {
        Self::new(true, true, true)
    }

    pub const fn web() -> Self {
        Self::new(true, true, false)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrohBootstrapBundle {
    pub protocol_version: u16,
    pub endpoint_ticket: EndpointTicket,
    pub capabilities: PeerCapabilities,
    pub device_label: Option<String>,
    pub session_nonce: [u8; 16],
    pub expires_at_unix_secs: u64,
}

impl IrohBootstrapBundle {
    pub fn new(
        endpoint_ticket: EndpointTicket,
        capabilities: PeerCapabilities,
        device_label: Option<String>,
        expires_at_unix_secs: u64,
    ) -> Self {
        let mut session_nonce = [0u8; 16];
        OsRng.fill_bytes(&mut session_nonce);
        Self::with_nonce(
            endpoint_ticket,
            capabilities,
            device_label,
            session_nonce,
            expires_at_unix_secs,
        )
    }

    pub fn with_nonce(
        endpoint_ticket: EndpointTicket,
        capabilities: PeerCapabilities,
        device_label: Option<String>,
        session_nonce: [u8; 16],
        expires_at_unix_secs: u64,
    ) -> Self {
        Self {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            endpoint_ticket,
            capabilities,
            device_label,
            session_nonce,
            expires_at_unix_secs,
        }
    }

    pub fn binding_material(&self) -> Vec<u8> {
        let mut material = Vec::new();
        material.extend_from_slice(&self.protocol_version.to_be_bytes());
        material.extend_from_slice(self.endpoint_ticket.to_string().as_bytes());
        material.extend_from_slice(&self.session_nonce);
        material.extend_from_slice(&self.expires_at_unix_secs.to_be_bytes());
        material.push(self.capabilities.messages as u8);
        material.push(self.capabilities.files as u8);
        material.push(self.capabilities.folder_sync as u8);
        if let Some(label) = &self.device_label {
            material.extend_from_slice(label.as_bytes());
        }
        material
    }
}

pub const CURRENT_PROTOCOL_VERSION: u16 = 1;
