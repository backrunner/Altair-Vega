pub mod bootstrap;
pub mod code;
pub mod pairing;
pub mod probe;
pub mod rendezvous;

pub use bootstrap::{CURRENT_PROTOCOL_VERSION, IrohBootstrapBundle, PeerCapabilities};
pub use code::{ShortCode, ShortCodeError};
pub use pairing::{
    EstablishedPairing, PairingError, PairingHandshake, PairingIntroEnvelope, PairingPhase,
};
pub use probe::{PairingProbeOutcome, run_local_pairing_probe};
pub use rendezvous::{
    ClientMessage as RendezvousClientMessage, JoinRequest as RendezvousJoinRequest,
    RendezvousErrorCode, ServerMessage as RendezvousServerMessage,
};

pub const PROTOCOL_NAME: &str = "altair-vega";
