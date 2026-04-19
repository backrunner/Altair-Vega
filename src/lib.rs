pub mod bootstrap;
pub mod code;
pub mod control;
pub mod files;
pub mod messaging;
pub mod pairing;
pub mod probe;
pub mod rendezvous;

pub use bootstrap::{CURRENT_PROTOCOL_VERSION, IrohBootstrapBundle, PeerCapabilities};
pub use code::{ShortCode, ShortCodeError};
pub use control::{CONTROL_ALPN, ChatMessage, ControlFrame, ControlSession, MessagingPeerKind};
pub use control::{
    FileChunkRange, FileDescriptor, FileOffer, FileProgress, FileProgressPhase, FileResponse,
    FileResumeInfo, FileTicket, FileTransport,
};
pub use files::{
    FileProbeConfig, FileProbeMode, FileProbeOutcome, NativeResumeProbeOutcome,
    run_local_file_probe, run_local_file_probe_with_config, run_local_native_resume_probe,
};
pub use messaging::{MessagingProbeOutcome, run_local_message_probe};
pub use pairing::{
    EstablishedPairing, PairingError, PairingHandshake, PairingIntroEnvelope, PairingPhase,
};
pub use probe::{PairingProbeOutcome, run_local_pairing_probe};
pub use rendezvous::{
    ClientMessage as RendezvousClientMessage, JoinRequest as RendezvousJoinRequest,
    RendezvousErrorCode, ServerMessage as RendezvousServerMessage,
};

pub const PROTOCOL_NAME: &str = "altair-vega";
