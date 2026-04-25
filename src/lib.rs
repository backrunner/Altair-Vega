pub mod bootstrap;
pub mod code;
pub mod control;
#[cfg(not(target_arch = "wasm32"))]
pub mod files;
#[cfg(not(target_arch = "wasm32"))]
pub mod messaging;
pub mod pairing;
#[cfg(not(target_arch = "wasm32"))]
pub mod probe;
pub mod rendezvous;
#[cfg(not(target_arch = "wasm32"))]
pub mod runtime;
#[cfg(not(target_arch = "wasm32"))]
pub mod sync;

pub use bootstrap::{CURRENT_PROTOCOL_VERSION, IrohBootstrapBundle, PeerCapabilities};
pub use code::{ShortCode, ShortCodeError};
pub use control::{CONTROL_ALPN, ChatMessage, ControlFrame, ControlSession, MessagingPeerKind};
pub use control::{
    FileChunkRange, FileDescriptor, FileOffer, FileProgress, FileProgressPhase, FileResponse,
    FileResumeInfo, FileTicket, FileTransport,
};
#[cfg(not(target_arch = "wasm32"))]
pub use files::{
    FileProbeConfig, FileProbeMode, FileProbeOutcome, NativeResumeProbeOutcome,
    run_local_file_probe, run_local_file_probe_with_config, run_local_native_resume_probe,
};
#[cfg(not(target_arch = "wasm32"))]
pub use messaging::{MessagingProbeOutcome, run_local_message_probe};
pub use pairing::{
    EstablishedPairing, PairingError, PairingHandshake, PairingIntroEnvelope, PairingPhase,
};
#[cfg(not(target_arch = "wasm32"))]
pub use probe::{PairingProbeOutcome, run_local_pairing_probe};
pub use rendezvous::{
    ClientMessage as RendezvousClientMessage, JoinRequest as RendezvousJoinRequest,
    RendezvousErrorCode, ServerMessage as RendezvousServerMessage,
};
#[cfg(not(target_arch = "wasm32"))]
pub use runtime::{
    DisposableRuntime, KEEP_RUNTIME_ENV, RUNTIME_ROOT_ENV, keep_runtime_requested,
    preferred_runtime_parent, resolve_runtime_state_dir, runtime_root_from_env,
};
#[cfg(not(target_arch = "wasm32"))]
pub use sync::{
    DEFAULT_SYNC_CHUNK_SIZE_BYTES, SyncAction, SyncChange, SyncChangeKind, SyncConflict,
    SyncConflictResolution, SyncEntry, SyncEntryState, SyncManifest, SyncMergePlan,
    apply_merge_plan, conflict_copy_path, diff_manifests, join_sync_path, manifests_state_eq,
    merge_manifests, scan_directory, sync_apply_target_path, sync_temp_path, unix_time_now_ms,
    validate_sync_manifest, with_tombstones,
};

pub const PROTOCOL_NAME: &str = "altair-vega";
