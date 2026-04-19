use crate::{IrohBootstrapBundle, ShortCode};
use blake3::Hasher;
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::time::{Duration, SystemTime};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingPhase {
    AwaitingPeerMessage,
    Established,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingIntroEnvelope {
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EstablishedPairing {
    code: ShortCode,
    expires_at: SystemTime,
    session_key: [u8; 32],
}

#[derive(Debug)]
pub struct PairingHandshake {
    code: ShortCode,
    expires_at: SystemTime,
    outbound_pake_message: Vec<u8>,
    phase: PairingPhase,
    spake: Option<Spake2<Ed25519Group>>,
    established: Option<EstablishedPairing>,
}

#[derive(Debug, Error)]
pub enum PairingError {
    #[error("pairing session expired")]
    Expired,
    #[error("pairing session already established")]
    AlreadyEstablished,
    #[error("failed to finish SPAKE2 exchange: {0:?}")]
    Spake(spake2::Error),
    #[error("failed to serialize bootstrap bundle")]
    Serialize(#[from] serde_json::Error),
    #[error("failed to encrypt bootstrap bundle")]
    Encrypt,
    #[error("failed to decrypt bootstrap bundle")]
    Decrypt,
}

impl PairingHandshake {
    pub fn new(code: ShortCode, now: SystemTime, ttl: Duration) -> Self {
        let password = Password::new(code.secret_phrase().as_bytes());
        let identity = code.pairing_identity();
        let (spake, outbound_pake_message) =
            Spake2::<Ed25519Group>::start_symmetric(&password, &Identity::new(identity.as_bytes()));

        Self {
            code,
            expires_at: now + ttl,
            outbound_pake_message,
            phase: PairingPhase::AwaitingPeerMessage,
            spake: Some(spake),
            established: None,
        }
    }

    pub fn code(&self) -> &ShortCode {
        &self.code
    }

    pub fn expires_at(&self) -> SystemTime {
        self.expires_at
    }

    pub fn phase(&self) -> PairingPhase {
        self.phase
    }

    pub fn outbound_pake_message(&self) -> &[u8] {
        &self.outbound_pake_message
    }

    pub fn established(&self) -> Option<&EstablishedPairing> {
        self.established.as_ref()
    }

    pub fn finish(
        &mut self,
        peer_message: &[u8],
        now: SystemTime,
    ) -> Result<&EstablishedPairing, PairingError> {
        if now > self.expires_at {
            return Err(PairingError::Expired);
        }
        if self.phase == PairingPhase::Established {
            return Err(PairingError::AlreadyEstablished);
        }

        let spake = self.spake.take().ok_or(PairingError::AlreadyEstablished)?;
        let shared_secret = spake.finish(peer_message).map_err(PairingError::Spake)?;
        let session_key = blake3::derive_key("altair-vega/pairing/session-v1", &shared_secret);

        self.phase = PairingPhase::Established;
        self.established = Some(EstablishedPairing {
            code: self.code.clone(),
            expires_at: self.expires_at,
            session_key,
        });

        Ok(self.established.as_ref().expect("established pairing set"))
    }
}

impl EstablishedPairing {
    pub fn code(&self) -> &ShortCode {
        &self.code
    }

    pub fn expires_at(&self) -> SystemTime {
        self.expires_at
    }

    pub fn session_key(&self) -> &[u8; 32] {
        &self.session_key
    }

    pub fn seal_bootstrap(
        &self,
        bundle: &IrohBootstrapBundle,
    ) -> Result<PairingIntroEnvelope, PairingError> {
        let plaintext = serde_json::to_vec(bundle)?;
        let aad = self.code.normalized();
        let cipher = self.intro_cipher();
        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);

        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| PairingError::Encrypt)?;

        Ok(PairingIntroEnvelope { nonce, ciphertext })
    }

    pub fn open_bootstrap(
        &self,
        envelope: &PairingIntroEnvelope,
    ) -> Result<IrohBootstrapBundle, PairingError> {
        let aad = self.code.normalized();
        let plaintext = self
            .intro_cipher()
            .decrypt(
                XNonce::from_slice(&envelope.nonce),
                Payload {
                    msg: &envelope.ciphertext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| PairingError::Decrypt)?;

        Ok(serde_json::from_slice(&plaintext)?)
    }

    pub fn connection_binding_tag(&self, bundle: &IrohBootstrapBundle) -> [u8; 16] {
        let binding_key = blake3::derive_key("altair-vega/pairing/binding-v1", &self.session_key);
        let mut hasher = Hasher::new_keyed(&binding_key);
        hasher.update(&bundle.binding_material());
        let hash = hasher.finalize();
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&hash.as_bytes()[..16]);
        tag
    }

    fn intro_cipher(&self) -> XChaCha20Poly1305 {
        let key = blake3::derive_key("altair-vega/pairing/intro-v1", &self.session_key);
        XChaCha20Poly1305::new_from_slice(&key).expect("derived key has valid length")
    }
}

#[cfg(test)]
mod tests {
    use super::{PairingError, PairingHandshake, PairingPhase};
    use crate::ShortCode;
    use std::{str::FromStr, time::Duration};

    fn now() -> std::time::SystemTime {
        std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    #[test]
    fn symmetric_handshake_derives_the_same_key() {
        let code = ShortCode::from_str("2048-badar-celen-votun").unwrap();
        let mut left = PairingHandshake::new(code.clone(), now(), Duration::from_secs(60));
        let mut right = PairingHandshake::new(code, now(), Duration::from_secs(60));
        let left_pake = left.outbound_pake_message().to_vec();
        let right_pake = right.outbound_pake_message().to_vec();

        let left_established = right.finish(&left_pake, now()).unwrap().clone();
        let right_established = left.finish(&right_pake, now()).unwrap().clone();

        assert_eq!(left.phase(), PairingPhase::Established);
        assert_eq!(right.phase(), PairingPhase::Established);
        assert_eq!(
            left_established.session_key(),
            right_established.session_key()
        );
    }

    #[test]
    fn rejects_replay_after_establishing() {
        let code = ShortCode::from_str("2048-badar-celen-votun").unwrap();
        let mut left = PairingHandshake::new(code.clone(), now(), Duration::from_secs(60));
        let mut right = PairingHandshake::new(code, now(), Duration::from_secs(60));
        let left_pake = left.outbound_pake_message().to_vec();
        let right_pake = right.outbound_pake_message().to_vec();

        right.finish(&left_pake, now()).unwrap();
        left.finish(&right_pake, now()).unwrap();

        let error = left.finish(&right_pake, now()).unwrap_err();
        assert!(matches!(error, PairingError::AlreadyEstablished));
    }

    #[test]
    fn rejects_expired_pairing() {
        let code = ShortCode::from_str("2048-badar-celen-votun").unwrap();
        let mut left = PairingHandshake::new(code.clone(), now(), Duration::from_secs(5));
        let right = PairingHandshake::new(code, now(), Duration::from_secs(5));
        let right_pake = right.outbound_pake_message().to_vec();

        let error = left
            .finish(&right_pake, now() + Duration::from_secs(10))
            .unwrap_err();
        assert!(matches!(error, PairingError::Expired));
    }
}
