use crate::{
    ControlFrame, ControlSession, IrohBootstrapBundle, MessagingPeerKind, PairingHandshake,
    ShortCode,
    control::{
        FileChunkRange, FileDescriptor, FileOffer, FileProgress, FileProgressPhase, FileResponse,
        FileResumeInfo, FileTicket, FileTransport,
    },
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use iroh::{Endpoint, endpoint::presets, protocol::Router};
use iroh_blobs::{
    BlobFormat, BlobsProtocol, Hash, HashAndFormat, store::fs::FsStore, ticket::BlobTicket,
};
use iroh_tickets::endpoint::EndpointTicket;
use rand::{RngCore, rngs::OsRng};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, SystemTime},
};
use tokio::io::AsyncReadExt;

const DEFAULT_CHUNK_SIZE_BYTES: u32 = 256 * 1024;
const CHUNK_HEADER_BYTES: usize = 8 + 4 + 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileProbeMode {
    Accept,
    Reject,
    Cancel,
    HashMismatch,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileProbeConfig {
    pub receiver_state_root: Option<PathBuf>,
    pub interrupt_after_chunks: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileProbeOutcome {
    pub code: ShortCode,
    pub left_kind: MessagingPeerKind,
    pub right_kind: MessagingPeerKind,
    pub file_name: String,
    pub transport: FileTransport,
    pub expected_hash: [u8; 32],
    pub received_hash: Option<[u8; 32]>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub accepted: bool,
    pub cancelled: bool,
    pub reason: Option<String>,
    pub resumed_local_bytes: u64,
    pub sender_progress: Vec<FileProgress>,
    pub receiver_progress: Vec<FileProgress>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeResumeProbeOutcome {
    pub code: ShortCode,
    pub file_name: String,
    pub seeded_chunks: u64,
    pub initial_local_bytes: u64,
    pub final_bytes: u64,
    pub expected_hash: [u8; 32],
    pub received_hash: [u8; 32],
}

#[derive(Clone, Debug)]
struct ReceiverTaskOutcome {
    received_hash: Option<[u8; 32]>,
    bytes_received: u64,
    reason: Option<String>,
    resumed_local_bytes: u64,
    progress: Vec<FileProgress>,
}

#[derive(Clone, Debug)]
struct FileProbeRunOptions {
    mode: FileProbeMode,
    receiver_state_root: Option<PathBuf>,
    interrupt_after_chunks: Option<u64>,
}

struct OwnedTempDir {
    path: PathBuf,
    cleanup: bool,
}

impl OwnedTempDir {
    fn new(prefix: &str) -> Result<Self> {
        let path = make_temp_dir(prefix)?;
        Ok(Self {
            path,
            cleanup: true,
        })
    }

    fn persistent(path: PathBuf) -> Result<Self> {
        fs::create_dir_all(&path)
            .with_context(|| format!("create persistent temp dir at {}", path.display()))?;
        Ok(Self {
            path,
            cleanup: false,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for OwnedTempDir {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

struct NativeBlobProvider {
    router: Router,
    ticket: BlobTicket,
}

impl NativeBlobProvider {
    async fn shutdown(self) -> Result<()> {
        self.router
            .shutdown()
            .await
            .context("shutdown native blob router")?;
        Ok(())
    }
}

struct PersistentChunkStore {
    root: PathBuf,
    descriptor: FileDescriptor,
}

impl PersistentChunkStore {
    fn load(root: impl AsRef<Path>, descriptor: &FileDescriptor) -> Result<Self> {
        let store = Self {
            root: root.as_ref().to_path_buf(),
            descriptor: descriptor.clone(),
        };
        fs::create_dir_all(store.chunks_dir()).with_context(|| {
            format!("create chunk store dir at {}", store.chunks_dir().display())
        })?;

        if store.meta_path().exists() {
            let bytes = fs::read(store.meta_path()).with_context(|| {
                format!(
                    "read chunk store metadata at {}",
                    store.meta_path().display()
                )
            })?;
            let existing: FileDescriptor =
                serde_json::from_slice(&bytes).context("deserialize chunk store metadata")?;
            ensure!(
                existing.hash == descriptor.hash,
                "existing chunk store hash does not match requested transfer"
            );
            ensure!(
                existing.size_bytes == descriptor.size_bytes,
                "existing chunk store size does not match requested transfer"
            );
            ensure!(
                existing.chunk_size_bytes == descriptor.chunk_size_bytes,
                "existing chunk store chunk size does not match requested transfer"
            );
        } else {
            let data =
                serde_json::to_vec_pretty(descriptor).context("serialize chunk store metadata")?;
            fs::write(store.meta_path(), data).with_context(|| {
                format!(
                    "write chunk store metadata at {}",
                    store.meta_path().display()
                )
            })?;
        }

        Ok(store)
    }

    fn local_bytes(&self) -> Result<u64> {
        let present = self.present_chunks()?;
        let mut total = 0u64;
        for (index, exists) in present.into_iter().enumerate() {
            if exists {
                total += self.expected_chunk_len(index as u64)? as u64;
            }
        }
        Ok(total)
    }

    fn resume_info(&self) -> Result<FileResumeInfo> {
        Ok(FileResumeInfo {
            chunk_size_bytes: self.descriptor.chunk_size_bytes,
            local_bytes: self.local_bytes()?,
            missing_ranges: self.missing_ranges()?,
        })
    }

    fn is_complete(&self) -> Result<bool> {
        Ok(self.missing_ranges()?.is_empty())
    }

    fn write_chunk(
        &self,
        index: u64,
        payload: &[u8],
        advertised_chunk_hash: [u8; 32],
    ) -> Result<()> {
        ensure!(index < self.chunk_count(), "chunk index out of bounds");
        ensure!(
            payload.len() == self.expected_chunk_len(index)?,
            "chunk payload length does not match expected size"
        );

        let actual_chunk_hash = *blake3::hash(payload).as_bytes();
        ensure!(
            actual_chunk_hash == advertised_chunk_hash,
            "chunk payload hash mismatch"
        );

        let path = self.chunk_path(index);
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, payload)
            .with_context(|| format!("write temporary chunk file at {}", tmp.display()))?;
        fs::rename(&tmp, &path)
            .with_context(|| format!("finalize chunk file at {}", path.display()))?;
        Ok(())
    }

    fn verify_complete(&self) -> Result<(u64, [u8; 32])> {
        ensure!(self.is_complete()?, "chunk store is not yet complete");
        let mut hasher = blake3::Hasher::new();
        let mut total = 0u64;

        for index in 0..self.chunk_count() {
            let path = self.chunk_path(index);
            let bytes = fs::read(&path)
                .with_context(|| format!("read chunk file at {}", path.display()))?;
            ensure!(
                bytes.len() == self.expected_chunk_len(index)?,
                "persisted chunk file has invalid length"
            );
            hasher.update(&bytes);
            total += bytes.len() as u64;
        }

        Ok((total, *hasher.finalize().as_bytes()))
    }

    fn clear(&self) -> Result<()> {
        if self.root.exists() {
            fs::remove_dir_all(&self.root).with_context(|| {
                format!("remove corrupt chunk store at {}", self.root.display())
            })?;
        }
        Ok(())
    }

    fn chunk_count(&self) -> u64 {
        if self.descriptor.size_bytes == 0 {
            0
        } else {
            self.descriptor
                .size_bytes
                .div_ceil(u64::from(self.descriptor.chunk_size_bytes))
        }
    }

    fn missing_ranges(&self) -> Result<Vec<FileChunkRange>> {
        let present = self.present_chunks()?;
        let mut missing = Vec::new();
        let mut start = None;

        for (index, exists) in present.into_iter().enumerate() {
            if !exists {
                start.get_or_insert(index as u64);
                continue;
            }

            if let Some(begin) = start.take() {
                missing.push(FileChunkRange {
                    start: begin,
                    end: index as u64,
                });
            }
        }

        if let Some(begin) = start {
            missing.push(FileChunkRange {
                start: begin,
                end: self.chunk_count(),
            });
        }

        Ok(missing)
    }

    fn present_chunks(&self) -> Result<Vec<bool>> {
        let mut present = vec![false; self.chunk_count() as usize];
        for index in 0..self.chunk_count() {
            let path = self.chunk_path(index);
            if !path.exists() {
                continue;
            }

            let expected_len = self.expected_chunk_len(index)? as u64;
            let actual_len = path
                .metadata()
                .with_context(|| format!("stat chunk file at {}", path.display()))?
                .len();
            if actual_len == expected_len {
                present[index as usize] = true;
            } else {
                fs::remove_file(&path)
                    .with_context(|| format!("remove invalid chunk file at {}", path.display()))?;
            }
        }
        Ok(present)
    }

    fn expected_chunk_len(&self, index: u64) -> Result<usize> {
        ensure!(index < self.chunk_count(), "chunk index out of bounds");
        let chunk_size = u64::from(self.descriptor.chunk_size_bytes);
        let start = index * chunk_size;
        let remaining = self.descriptor.size_bytes.saturating_sub(start);
        Ok(remaining.min(chunk_size) as usize)
    }

    fn meta_path(&self) -> PathBuf {
        self.root.join("meta.json")
    }

    fn chunks_dir(&self) -> PathBuf {
        self.root.join("chunks")
    }

    fn chunk_path(&self, index: u64) -> PathBuf {
        self.chunks_dir().join(format!("{index:020}.chunk"))
    }
}

pub async fn run_local_file_probe(
    code: ShortCode,
    left_kind: MessagingPeerKind,
    right_kind: MessagingPeerKind,
    file_name: impl Into<String>,
    payload: impl AsRef<[u8]>,
    mode: FileProbeMode,
) -> Result<FileProbeOutcome> {
    run_local_file_probe_with_config(
        code,
        left_kind,
        right_kind,
        file_name,
        payload,
        mode,
        FileProbeConfig::default(),
    )
    .await
}

pub async fn run_local_file_probe_with_config(
    code: ShortCode,
    left_kind: MessagingPeerKind,
    right_kind: MessagingPeerKind,
    file_name: impl Into<String>,
    payload: impl AsRef<[u8]>,
    mode: FileProbeMode,
    config: FileProbeConfig,
) -> Result<FileProbeOutcome> {
    run_local_file_probe_with_options(
        code,
        left_kind,
        right_kind,
        file_name,
        payload,
        FileProbeRunOptions {
            mode,
            receiver_state_root: config.receiver_state_root,
            interrupt_after_chunks: config.interrupt_after_chunks,
        },
    )
    .await
}

pub async fn run_local_native_resume_probe(
    code: ShortCode,
    file_name: impl Into<String>,
    payload: impl AsRef<[u8]>,
    seeded_chunks: u64,
    receiver_state_root: Option<PathBuf>,
) -> Result<NativeResumeProbeOutcome> {
    let file_name = file_name.into();
    let payload = payload.as_ref().to_vec();
    let expected_hash = *blake3::hash(&payload).as_bytes();
    let descriptor = FileDescriptor {
        name: file_name.clone(),
        size_bytes: payload.len() as u64,
        hash: expected_hash,
        chunk_size_bytes: DEFAULT_CHUNK_SIZE_BYTES,
    };

    let sender_root = OwnedTempDir::new("native-resume-sender")?;
    let sender_source_path = sender_root.path().join(&descriptor.name);
    fs::write(&sender_source_path, &payload).with_context(|| {
        format!(
            "write native resume source file at {}",
            sender_source_path.display()
        )
    })?;

    let receiver_root = match receiver_state_root {
        Some(path) => OwnedTempDir::persistent(path)?,
        None => OwnedTempDir::new("native-resume-receiver")?,
    };
    let receiver_store = receiver_root.path().join("native-store");

    let provider =
        spawn_native_blob_provider(&sender_source_path, &sender_root.path().join("provider"))
            .await
            .context("start native blob provider for resume probe")?;
    seed_native_blob_ranges(&receiver_store, &provider.ticket, seeded_chunks)
        .await
        .context("seed partial native blob ranges")?;
    let (initial_local_bytes, complete_before) = inspect_native_store(&receiver_store, &descriptor)
        .await
        .context("inspect seeded native resume state")?;
    ensure!(
        !complete_before,
        "native resume seed unexpectedly completed the blob"
    );

    let resumed = fetch_native_blob(&receiver_store, &provider.ticket)
        .await
        .context("resume native blob fetch after restart")?;
    provider.shutdown().await?;

    Ok(NativeResumeProbeOutcome {
        code,
        file_name,
        seeded_chunks,
        initial_local_bytes,
        final_bytes: resumed.bytes_received,
        expected_hash,
        received_hash: resumed.received_hash,
    })
}

async fn run_local_file_probe_with_options(
    code: ShortCode,
    left_kind: MessagingPeerKind,
    right_kind: MessagingPeerKind,
    file_name: impl Into<String>,
    payload: impl AsRef<[u8]>,
    options: FileProbeRunOptions,
) -> Result<FileProbeOutcome> {
    let file_name = file_name.into();
    let payload = payload.as_ref().to_vec();
    let actual_hash = *blake3::hash(&payload).as_bytes();
    let advertised_hash = if options.mode == FileProbeMode::HashMismatch {
        let mut tampered = actual_hash;
        tampered[0] ^= 0xff;
        tampered
    } else {
        actual_hash
    };
    let transport = choose_transport(left_kind, right_kind);
    let descriptor = FileDescriptor {
        name: file_name.clone(),
        size_bytes: payload.len() as u64,
        hash: advertised_hash,
        chunk_size_bytes: DEFAULT_CHUNK_SIZE_BYTES,
    };

    let sender_root = OwnedTempDir::new("sender-transfer")?;
    let sender_source_path = sender_root.path().join(&descriptor.name);
    fs::write(&sender_source_path, &payload).with_context(|| {
        format!(
            "write sender source file at {}",
            sender_source_path.display()
        )
    })?;

    let receiver_root = match options.receiver_state_root.clone() {
        Some(path) => OwnedTempDir::persistent(path)?,
        None => OwnedTempDir::new("receiver-transfer")?,
    };

    let now = SystemTime::now();
    let ttl = Duration::from_secs(60);
    let expires_at = unix_secs(now + ttl);

    let left = Endpoint::builder(presets::N0)
        .alpns(vec![crate::CONTROL_ALPN.to_vec()])
        .bind()
        .await
        .context("bind left file control endpoint")?;
    let right = Endpoint::builder(presets::N0)
        .alpns(vec![crate::CONTROL_ALPN.to_vec()])
        .bind()
        .await
        .context("bind right file control endpoint")?;

    let left_bundle = IrohBootstrapBundle::new(
        EndpointTicket::new(left.addr()),
        left_kind.capabilities(),
        Some(left_kind.label().to_string()),
        expires_at,
    );
    let right_bundle = IrohBootstrapBundle::new(
        EndpointTicket::new(right.addr()),
        right_kind.capabilities(),
        Some(right_kind.label().to_string()),
        expires_at,
    );

    let (left_pairing, right_pairing) = exchange_pairing(code.clone(), now, ttl)?;
    let left_remote_bundle = right_pairing
        .open_bootstrap(&left_pairing.seal_bootstrap(&left_bundle)?)
        .context("open left bootstrap on right peer")?;
    let right_remote_bundle = left_pairing
        .open_bootstrap(&right_pairing.seal_bootstrap(&right_bundle)?)
        .context("open right bootstrap on left peer")?;

    let offer = FileOffer {
        transfer_id: u64::from_be_bytes(
            left_bundle.session_nonce[..8]
                .try_into()
                .expect("nonce size is correct"),
        ),
        descriptor: descriptor.clone(),
        transport,
    };
    let expected_file_name = offer.descriptor.name.clone();

    let right_task = tokio::spawn({
        let right = right.clone();
        let right_pairing = right_pairing.clone();
        let right_bundle = right_bundle.clone();
        let left_remote_bundle = left_remote_bundle.clone();
        let receiver_root = receiver_root.path().to_path_buf();
        let mode = options.mode;
        async move {
            let incoming = right.accept().await.ok_or_else(|| {
                anyhow!("right endpoint closed before accepting file control connection")
            })?;
            let connection = incoming
                .accept()
                .context("accept right file control connection")?
                .await
                .context("complete right file control connection")?;
            let mut session = ControlSession::accept(
                connection,
                &right_pairing,
                &right_bundle,
                &left_remote_bundle,
            )
            .await
            .context("accept file control session")?;

            let Some(ControlFrame::FileOffer(remote_offer)) = session.receive_frame().await? else {
                bail!("expected file offer frame as first file control message");
            };
            ensure!(
                remote_offer.descriptor.name == expected_file_name,
                "unexpected offered file name"
            );

            let receiver_progress = vec![FileProgress {
                transfer_id: remote_offer.transfer_id,
                phase: FileProgressPhase::Offered,
                bytes_complete: 0,
                total_bytes: remote_offer.descriptor.size_bytes,
            }];

            match mode {
                FileProbeMode::Reject => {
                    return reject_offer(session, remote_offer, receiver_progress).await;
                }
                FileProbeMode::Cancel => {
                    return cancel_offer(session, remote_offer, receiver_progress).await;
                }
                FileProbeMode::Accept | FileProbeMode::HashMismatch => {}
            }

            match remote_offer.transport {
                FileTransport::NativeBlob => {
                    handle_native_receiver(
                        session,
                        &remote_offer,
                        &receiver_root,
                        receiver_progress,
                    )
                    .await
                }
                FileTransport::ChunkedStream => {
                    handle_chunked_receiver(
                        session,
                        &remote_offer,
                        &receiver_root,
                        receiver_progress,
                    )
                    .await
                }
            }
        }
    });

    let mut sender_progress = vec![FileProgress {
        transfer_id: offer.transfer_id,
        phase: FileProgressPhase::Offered,
        bytes_complete: 0,
        total_bytes: descriptor.size_bytes,
    }];

    let mut session =
        ControlSession::connect(&left, &left_pairing, &left_bundle, &right_remote_bundle)
            .await
            .context("connect file control session")?;
    session
        .send_frame(ControlFrame::FileOffer(offer.clone()))
        .await
        .context("send file offer")?;

    let response = session
        .receive_frame()
        .await
        .context("receive file response")?;
    let resumed_local_bytes = match &response {
        Some(ControlFrame::FileResponse(response)) => response
            .resume
            .as_ref()
            .map(|resume| resume.local_bytes)
            .unwrap_or(0),
        _ => 0,
    };
    match response {
        Some(ControlFrame::FileResponse(response)) if response.accepted => {
            sender_progress.push(FileProgress {
                transfer_id: offer.transfer_id,
                phase: FileProgressPhase::Accepted,
                bytes_complete: resumed_local_bytes,
                total_bytes: descriptor.size_bytes,
            });

            let terminal = match transport {
                FileTransport::NativeBlob => {
                    if resumed_local_bytes < descriptor.size_bytes {
                        let provider = spawn_native_blob_provider(
                            &sender_source_path,
                            &sender_root.path().join("native-provider"),
                        )
                        .await
                        .context("start native blob provider")?;

                        sender_progress.push(FileProgress {
                            transfer_id: offer.transfer_id,
                            phase: FileProgressPhase::Sending,
                            bytes_complete: descriptor
                                .size_bytes
                                .saturating_sub(resumed_local_bytes),
                            total_bytes: descriptor.size_bytes,
                        });

                        session
                            .send_frame(ControlFrame::FileTicket(FileTicket {
                                transfer_id: offer.transfer_id,
                                ticket: provider.ticket.to_string(),
                            }))
                            .await
                            .context("send native blob ticket")?;
                        let terminal = session
                            .receive_frame()
                            .await
                            .context("receive native transfer terminal frame")?;
                        provider.shutdown().await?;
                        terminal
                    } else {
                        session
                            .receive_frame()
                            .await
                            .context("receive native no-op terminal frame")?
                    }
                }
                FileTransport::ChunkedStream => {
                    let resume = response.resume.ok_or_else(|| {
                        anyhow!("chunked transfer response did not include resume metadata")
                    })?;

                    let bytes_sent = if resume.missing_ranges.is_empty() {
                        0
                    } else {
                        send_chunk_stream(
                            &mut session,
                            &sender_source_path,
                            &descriptor,
                            &resume.missing_ranges,
                            &mut sender_progress,
                            options.interrupt_after_chunks,
                        )
                        .await
                        .context("send chunked file payload stream")?
                    };
                    let _ = bytes_sent;
                    session
                        .receive_frame()
                        .await
                        .context("receive chunked transfer terminal frame")?
                }
            };

            let right_outcome = right_task.await.context("join file receiver task")??;
            let (accepted, cancelled, terminal_reason) = match terminal {
                Some(ControlFrame::FileProgress(progress))
                    if progress.phase == FileProgressPhase::Completed =>
                {
                    sender_progress.push(progress);
                    (true, false, None)
                }
                Some(ControlFrame::FileCancel { reason, .. }) => {
                    sender_progress.push(FileProgress {
                        transfer_id: offer.transfer_id,
                        phase: FileProgressPhase::Cancelled,
                        bytes_complete: right_outcome.bytes_received,
                        total_bytes: descriptor.size_bytes,
                    });
                    (false, true, Some(reason))
                }
                other => bail!("unexpected terminal file frame: {other:?}"),
            };

            session.finish_sending()?;
            session.wait_for_send_completion().await?;
            left.close().await;
            right.close().await;

            let bytes_sent = match transport {
                FileTransport::NativeBlob => {
                    descriptor.size_bytes.saturating_sub(resumed_local_bytes)
                }
                FileTransport::ChunkedStream => sender_progress
                    .iter()
                    .rfind(|item| item.phase == FileProgressPhase::Sending)
                    .map(|item| item.bytes_complete)
                    .unwrap_or(0),
            };

            return Ok(FileProbeOutcome {
                code,
                left_kind,
                right_kind,
                file_name,
                transport,
                expected_hash: advertised_hash,
                received_hash: right_outcome.received_hash,
                bytes_sent,
                bytes_received: right_outcome.bytes_received,
                accepted,
                cancelled,
                reason: terminal_reason.or(right_outcome.reason),
                resumed_local_bytes: right_outcome.resumed_local_bytes.max(resumed_local_bytes),
                sender_progress,
                receiver_progress: right_outcome.progress,
            });
        }
        Some(ControlFrame::FileResponse(_response)) => {}
        Some(ControlFrame::FileCancel { reason, .. }) => {
            session.finish_sending()?;
            session.wait_for_send_completion().await?;
            let outcome = right_task.await.context("join cancelled receiver task")??;
            left.close().await;
            right.close().await;
            return Ok(FileProbeOutcome {
                code,
                left_kind,
                right_kind,
                file_name,
                transport,
                expected_hash: advertised_hash,
                received_hash: outcome.received_hash,
                bytes_sent: 0,
                bytes_received: outcome.bytes_received,
                accepted: false,
                cancelled: true,
                reason: Some(reason),
                resumed_local_bytes: outcome.resumed_local_bytes,
                sender_progress,
                receiver_progress: outcome.progress,
            });
        }
        other => bail!("unexpected file response frame: {other:?}"),
    }

    session.finish_sending()?;
    session.wait_for_send_completion().await?;
    let outcome = right_task.await.context("join rejected receiver task")??;
    left.close().await;
    right.close().await;
    sender_progress.push(FileProgress {
        transfer_id: offer.transfer_id,
        phase: FileProgressPhase::Cancelled,
        bytes_complete: resumed_local_bytes,
        total_bytes: descriptor.size_bytes,
    });

    Ok(FileProbeOutcome {
        code,
        left_kind,
        right_kind,
        file_name,
        transport,
        expected_hash: advertised_hash,
        received_hash: outcome.received_hash,
        bytes_sent: 0,
        bytes_received: outcome.bytes_received,
        accepted: false,
        cancelled: true,
        reason: outcome.reason,
        resumed_local_bytes: outcome.resumed_local_bytes,
        sender_progress,
        receiver_progress: outcome.progress,
    })
}

fn choose_transport(left_kind: MessagingPeerKind, right_kind: MessagingPeerKind) -> FileTransport {
    if left_kind == MessagingPeerKind::Cli && right_kind == MessagingPeerKind::Cli {
        FileTransport::NativeBlob
    } else {
        FileTransport::ChunkedStream
    }
}

async fn reject_offer(
    mut session: ControlSession,
    offer: FileOffer,
    mut progress: Vec<FileProgress>,
) -> Result<ReceiverTaskOutcome> {
    session
        .send_frame(ControlFrame::FileResponse(FileResponse {
            transfer_id: offer.transfer_id,
            accepted: false,
            reason: Some("receiver rejected transfer".to_string()),
            resume: Some(FileResumeInfo {
                chunk_size_bytes: offer.descriptor.chunk_size_bytes,
                local_bytes: 0,
                missing_ranges: Vec::new(),
            }),
        }))
        .await?;
    session.finish_sending()?;
    session.wait_for_send_completion().await?;
    progress.push(FileProgress {
        transfer_id: offer.transfer_id,
        phase: FileProgressPhase::Cancelled,
        bytes_complete: 0,
        total_bytes: offer.descriptor.size_bytes,
    });
    Ok(ReceiverTaskOutcome {
        received_hash: None,
        bytes_received: 0,
        reason: Some("receiver rejected transfer".to_string()),
        resumed_local_bytes: 0,
        progress,
    })
}

async fn cancel_offer(
    mut session: ControlSession,
    offer: FileOffer,
    mut progress: Vec<FileProgress>,
) -> Result<ReceiverTaskOutcome> {
    session
        .send_frame(ControlFrame::FileCancel {
            transfer_id: offer.transfer_id,
            reason: "receiver cancelled transfer".to_string(),
        })
        .await?;
    session.finish_sending()?;
    session.wait_for_send_completion().await?;
    progress.push(FileProgress {
        transfer_id: offer.transfer_id,
        phase: FileProgressPhase::Cancelled,
        bytes_complete: 0,
        total_bytes: offer.descriptor.size_bytes,
    });
    Ok(ReceiverTaskOutcome {
        received_hash: None,
        bytes_received: 0,
        reason: Some("receiver cancelled transfer".to_string()),
        resumed_local_bytes: 0,
        progress,
    })
}

async fn handle_native_receiver(
    mut session: ControlSession,
    offer: &FileOffer,
    receiver_root: &Path,
    mut progress: Vec<FileProgress>,
) -> Result<ReceiverTaskOutcome> {
    let native_store_dir = receiver_root.join("native-store");
    let (resume_local_bytes, complete_before) =
        inspect_native_store(&native_store_dir, &offer.descriptor)
            .await
            .context("inspect native blob resume state")?;

    session
        .send_frame(ControlFrame::FileResponse(FileResponse {
            transfer_id: offer.transfer_id,
            accepted: true,
            reason: None,
            resume: Some(FileResumeInfo {
                chunk_size_bytes: offer.descriptor.chunk_size_bytes,
                local_bytes: resume_local_bytes,
                missing_ranges: Vec::new(),
            }),
        }))
        .await?;
    progress.push(FileProgress {
        transfer_id: offer.transfer_id,
        phase: FileProgressPhase::Accepted,
        bytes_complete: resume_local_bytes,
        total_bytes: offer.descriptor.size_bytes,
    });

    if complete_before {
        let (bytes_received, received_hash) =
            verify_native_store(&native_store_dir, &offer.descriptor)
                .await
                .context("verify already-complete native blob store")?;
        session
            .send_frame(ControlFrame::FileProgress(FileProgress {
                transfer_id: offer.transfer_id,
                phase: FileProgressPhase::Completed,
                bytes_complete: bytes_received,
                total_bytes: offer.descriptor.size_bytes,
            }))
            .await?;
        session.finish_sending()?;
        session.wait_for_send_completion().await?;
        progress.push(FileProgress {
            transfer_id: offer.transfer_id,
            phase: FileProgressPhase::Completed,
            bytes_complete: bytes_received,
            total_bytes: offer.descriptor.size_bytes,
        });
        return Ok(ReceiverTaskOutcome {
            received_hash: Some(received_hash),
            bytes_received,
            reason: None,
            resumed_local_bytes: resume_local_bytes,
            progress,
        });
    }

    let Some(ControlFrame::FileTicket(file_ticket)) = session.receive_frame().await? else {
        bail!("expected native blob ticket after acceptance");
    };
    ensure!(
        file_ticket.transfer_id == offer.transfer_id,
        "native blob ticket transfer id mismatch"
    );

    let blob_ticket = BlobTicket::from_str(&file_ticket.ticket).context("parse blob ticket")?;
    let fetch = fetch_native_blob(&native_store_dir, &blob_ticket)
        .await
        .context("fetch native blob content")?;
    progress.push(FileProgress {
        transfer_id: offer.transfer_id,
        phase: FileProgressPhase::Sending,
        bytes_complete: fetch.bytes_received,
        total_bytes: offer.descriptor.size_bytes,
    });
    progress.push(FileProgress {
        transfer_id: offer.transfer_id,
        phase: FileProgressPhase::Verifying,
        bytes_complete: fetch.bytes_received,
        total_bytes: offer.descriptor.size_bytes,
    });

    if fetch.received_hash != offer.descriptor.hash {
        session
            .send_frame(ControlFrame::FileCancel {
                transfer_id: offer.transfer_id,
                reason: "file hash mismatch".to_string(),
            })
            .await?;
        session.finish_sending()?;
        session.wait_for_send_completion().await?;
        progress.push(FileProgress {
            transfer_id: offer.transfer_id,
            phase: FileProgressPhase::Cancelled,
            bytes_complete: fetch.bytes_received,
            total_bytes: offer.descriptor.size_bytes,
        });
        return Ok(ReceiverTaskOutcome {
            received_hash: Some(fetch.received_hash),
            bytes_received: fetch.bytes_received,
            reason: Some("file hash mismatch".to_string()),
            resumed_local_bytes: resume_local_bytes,
            progress,
        });
    }

    session
        .send_frame(ControlFrame::FileProgress(FileProgress {
            transfer_id: offer.transfer_id,
            phase: FileProgressPhase::Completed,
            bytes_complete: fetch.bytes_received,
            total_bytes: offer.descriptor.size_bytes,
        }))
        .await?;
    session.finish_sending()?;
    session.wait_for_send_completion().await?;
    progress.push(FileProgress {
        transfer_id: offer.transfer_id,
        phase: FileProgressPhase::Completed,
        bytes_complete: fetch.bytes_received,
        total_bytes: offer.descriptor.size_bytes,
    });

    Ok(ReceiverTaskOutcome {
        received_hash: Some(fetch.received_hash),
        bytes_received: fetch.bytes_received,
        reason: None,
        resumed_local_bytes: resume_local_bytes,
        progress,
    })
}

async fn handle_chunked_receiver(
    mut session: ControlSession,
    offer: &FileOffer,
    receiver_root: &Path,
    mut progress: Vec<FileProgress>,
) -> Result<ReceiverTaskOutcome> {
    let store = PersistentChunkStore::load(
        receiver_root
            .join("chunk-store")
            .join(hash_to_hex(offer.descriptor.hash)),
        &offer.descriptor,
    )
    .context("load persistent chunk store")?;
    let resume = store.resume_info()?;

    session
        .send_frame(ControlFrame::FileResponse(FileResponse {
            transfer_id: offer.transfer_id,
            accepted: true,
            reason: None,
            resume: Some(resume.clone()),
        }))
        .await?;
    progress.push(FileProgress {
        transfer_id: offer.transfer_id,
        phase: FileProgressPhase::Accepted,
        bytes_complete: resume.local_bytes,
        total_bytes: offer.descriptor.size_bytes,
    });

    if !resume.missing_ranges.is_empty() {
        let mut recv = session.accept_uni().await?;
        let bytes_received =
            receive_chunk_stream(&mut recv, &store, &offer.descriptor, &mut progress)
                .await
                .context("receive chunk stream payload")?;
        let _ = bytes_received;
    }

    if !store.is_complete()? {
        let local_bytes = store.local_bytes()?;
        session
            .send_frame(ControlFrame::FileCancel {
                transfer_id: offer.transfer_id,
                reason: "transfer interrupted before completion".to_string(),
            })
            .await?;
        session.finish_sending()?;
        session.wait_for_send_completion().await?;
        progress.push(FileProgress {
            transfer_id: offer.transfer_id,
            phase: FileProgressPhase::Cancelled,
            bytes_complete: local_bytes,
            total_bytes: offer.descriptor.size_bytes,
        });
        return Ok(ReceiverTaskOutcome {
            received_hash: None,
            bytes_received: local_bytes,
            reason: Some("transfer interrupted before completion".to_string()),
            resumed_local_bytes: resume.local_bytes,
            progress,
        });
    }

    let (bytes_received, received_hash) = store.verify_complete()?;
    progress.push(FileProgress {
        transfer_id: offer.transfer_id,
        phase: FileProgressPhase::Verifying,
        bytes_complete: bytes_received,
        total_bytes: offer.descriptor.size_bytes,
    });

    if received_hash != offer.descriptor.hash {
        store.clear()?;
        session
            .send_frame(ControlFrame::FileCancel {
                transfer_id: offer.transfer_id,
                reason: "file hash mismatch".to_string(),
            })
            .await?;
        session.finish_sending()?;
        session.wait_for_send_completion().await?;
        progress.push(FileProgress {
            transfer_id: offer.transfer_id,
            phase: FileProgressPhase::Cancelled,
            bytes_complete: bytes_received,
            total_bytes: offer.descriptor.size_bytes,
        });
        return Ok(ReceiverTaskOutcome {
            received_hash: Some(received_hash),
            bytes_received,
            reason: Some("file hash mismatch".to_string()),
            resumed_local_bytes: resume.local_bytes,
            progress,
        });
    }

    session
        .send_frame(ControlFrame::FileProgress(FileProgress {
            transfer_id: offer.transfer_id,
            phase: FileProgressPhase::Completed,
            bytes_complete: bytes_received,
            total_bytes: offer.descriptor.size_bytes,
        }))
        .await?;
    session.finish_sending()?;
    session.wait_for_send_completion().await?;
    progress.push(FileProgress {
        transfer_id: offer.transfer_id,
        phase: FileProgressPhase::Completed,
        bytes_complete: bytes_received,
        total_bytes: offer.descriptor.size_bytes,
    });

    Ok(ReceiverTaskOutcome {
        received_hash: Some(received_hash),
        bytes_received,
        reason: None,
        resumed_local_bytes: resume.local_bytes,
        progress,
    })
}

async fn send_chunk_stream(
    session: &mut ControlSession,
    source_path: &Path,
    descriptor: &FileDescriptor,
    missing_ranges: &[FileChunkRange],
    sender_progress: &mut Vec<FileProgress>,
    interrupt_after_chunks: Option<u64>,
) -> Result<u64> {
    let mut stream = session.open_uni().await?;
    let mut file = fs::File::open(source_path)
        .with_context(|| format!("open source file at {}", source_path.display()))?;
    let mut bytes_sent = 0u64;
    let mut sent_chunks = 0u64;
    let transfer_id = sender_progress[0].transfer_id;

    for range in missing_ranges {
        for index in range.start..range.end {
            if interrupt_after_chunks.is_some_and(|limit| sent_chunks >= limit) {
                stream.finish().context("finish interrupted chunk stream")?;
                let _ = stream.stopped().await;
                return Ok(bytes_sent);
            }

            let chunk = read_file_chunk(source_path, &mut file, descriptor, index)
                .with_context(|| format!("read source chunk {index}"))?;
            let chunk_hash = *blake3::hash(&chunk).as_bytes();
            stream
                .write_all(&index.to_be_bytes())
                .await
                .context("write chunk index")?;
            stream
                .write_all(
                    &(u32::try_from(chunk.len()).context("chunk length overflow")?).to_be_bytes(),
                )
                .await
                .context("write chunk length")?;
            stream
                .write_all(&chunk_hash)
                .await
                .context("write chunk hash")?;
            stream
                .write_all(&chunk)
                .await
                .context("write chunk payload")?;

            bytes_sent += chunk.len() as u64;
            sent_chunks += 1;
            sender_progress.push(FileProgress {
                transfer_id,
                phase: FileProgressPhase::Sending,
                bytes_complete: bytes_sent,
                total_bytes: descriptor.size_bytes,
            });
        }
    }

    stream.finish().context("finish chunk stream")?;
    let _ = stream.stopped().await;
    Ok(bytes_sent)
}

async fn receive_chunk_stream(
    recv: &mut iroh::endpoint::RecvStream,
    store: &PersistentChunkStore,
    descriptor: &FileDescriptor,
    progress: &mut Vec<FileProgress>,
) -> Result<u64> {
    let mut bytes_received = store.local_bytes()?;
    loop {
        let mut header = [0u8; CHUNK_HEADER_BYTES];
        if !read_exact_or_eof(recv, &mut header)
            .await
            .context("read chunk stream header")?
        {
            return Ok(bytes_received);
        }

        let index = u64::from_be_bytes(
            header[..8]
                .try_into()
                .expect("chunk header index length is correct"),
        );
        let payload_len = u32::from_be_bytes(
            header[8..12]
                .try_into()
                .expect("chunk header length bytes are correct"),
        ) as usize;
        let chunk_hash: [u8; 32] = header[12..]
            .try_into()
            .expect("chunk header hash length is correct");
        let mut payload = vec![0u8; payload_len];
        read_exact_or_error(recv, &mut payload)
            .await
            .context("read chunk stream payload")?;
        store
            .write_chunk(index, &payload, chunk_hash)
            .with_context(|| format!("persist received chunk {index}"))?;
        bytes_received = store.local_bytes()?;
        progress.push(FileProgress {
            transfer_id: progress[0].transfer_id,
            phase: FileProgressPhase::Sending,
            bytes_complete: bytes_received,
            total_bytes: descriptor.size_bytes,
        });
    }
}

async fn spawn_native_blob_provider(
    source_path: &Path,
    provider_dir: &Path,
) -> Result<NativeBlobProvider> {
    fs::create_dir_all(provider_dir)
        .with_context(|| format!("create native provider dir at {}", provider_dir.display()))?;
    let store = FsStore::load(provider_dir)
        .await
        .with_context(|| format!("load native provider store at {}", provider_dir.display()))?;
    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("bind native blob provider endpoint")?;
    let blobs = BlobsProtocol::new(&store, None);
    let tag = blobs.add_path(source_path).await.with_context(|| {
        format!(
            "import source file into native blob store from {}",
            source_path.display()
        )
    })?;
    let router = Router::builder(endpoint.clone())
        .accept(iroh_blobs::ALPN, blobs.clone())
        .spawn();
    let ticket = BlobTicket::new(endpoint.addr(), tag.hash, tag.format);
    Ok(NativeBlobProvider { router, ticket })
}

async fn inspect_native_store(
    store_dir: &Path,
    descriptor: &FileDescriptor,
) -> Result<(u64, bool)> {
    fs::create_dir_all(store_dir)
        .with_context(|| format!("create native receiver dir at {}", store_dir.display()))?;
    let store = FsStore::load(store_dir)
        .await
        .with_context(|| format!("load native receiver store at {}", store_dir.display()))?;
    let local = store
        .remote()
        .local(native_hash_and_format(descriptor))
        .await
        .context("inspect native receiver store local info")?;
    let outcome = (local.local_bytes(), local.is_complete());
    store
        .shutdown()
        .await
        .context("shutdown inspected native store")?;
    Ok(outcome)
}

struct NativeFetchOutcome {
    bytes_received: u64,
    received_hash: [u8; 32],
}

async fn fetch_native_blob(store_dir: &Path, ticket: &BlobTicket) -> Result<NativeFetchOutcome> {
    let store = FsStore::load(store_dir)
        .await
        .with_context(|| format!("load native fetch store at {}", store_dir.display()))?;
    let local = store
        .remote()
        .local(ticket.hash_and_format())
        .await
        .context("inspect native store before fetch")?;

    if !local.is_complete() {
        let endpoint = Endpoint::bind(presets::N0)
            .await
            .context("bind native blob fetch endpoint")?;
        let connection = endpoint
            .connect(ticket.addr().clone(), iroh_blobs::ALPN)
            .await
            .context("connect to native blob provider")?;
        let request = local.missing();
        store
            .remote()
            .execute_get(connection, request)
            .await
            .context("execute native blob get request")?;
        endpoint.close().await;
    }

    let local_after = store
        .remote()
        .local(ticket.hash_and_format())
        .await
        .context("inspect native store after fetch")?;
    ensure!(
        local_after.is_complete(),
        "native blob fetch did not complete"
    );
    let bytes_received = local_after.local_bytes();
    let received_hash = hash_native_blob(&store, ticket.hash())
        .await
        .context("hash completed native blob fetch")?;
    store
        .shutdown()
        .await
        .context("shutdown native fetch store")?;
    Ok(NativeFetchOutcome {
        bytes_received,
        received_hash,
    })
}

async fn seed_native_blob_ranges(
    store_dir: &Path,
    ticket: &BlobTicket,
    chunk_count: u64,
) -> Result<u64> {
    use iroh_blobs::protocol::{ChunkRanges, ChunkRangesExt, GetRequest};

    fs::create_dir_all(store_dir)
        .with_context(|| format!("create native seed dir at {}", store_dir.display()))?;
    let store = FsStore::load(store_dir)
        .await
        .with_context(|| format!("load native seed store at {}", store_dir.display()))?;
    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("bind native partial seed endpoint")?;
    let connection = endpoint
        .connect(ticket.addr().clone(), iroh_blobs::ALPN)
        .await
        .context("connect to native provider for partial seed")?;
    let request = GetRequest::blob_ranges(ticket.hash(), ChunkRanges::chunks(..chunk_count));
    store
        .remote()
        .execute_get(connection, request)
        .await
        .context("execute partial native blob request")?;
    endpoint.close().await;
    let local = store
        .remote()
        .local(ticket.hash_and_format())
        .await
        .context("inspect native seed bytes")?;
    let local_bytes = local.local_bytes();
    store
        .shutdown()
        .await
        .context("shutdown native seed store")?;
    Ok(local_bytes)
}

async fn verify_native_store(
    store_dir: &Path,
    descriptor: &FileDescriptor,
) -> Result<(u64, [u8; 32])> {
    let store = FsStore::load(store_dir)
        .await
        .with_context(|| format!("reload native store at {}", store_dir.display()))?;
    let local = store
        .remote()
        .local(native_hash_and_format(descriptor))
        .await
        .context("inspect native store local completeness")?;
    ensure!(local.is_complete(), "native store is incomplete");
    let bytes_received = local.local_bytes();
    let hash = hash_native_blob(&store, descriptor.hash.into())
        .await
        .context("hash native blob payload")?;
    store
        .shutdown()
        .await
        .context("shutdown verified native store")?;
    Ok((bytes_received, hash))
}

async fn hash_native_blob(store: &FsStore, hash: Hash) -> Result<[u8; 32]> {
    let mut reader = store.reader(hash);
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buf)
            .await
            .context("read native blob bytes for hashing")?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(*hasher.finalize().as_bytes())
}

fn native_hash_and_format(descriptor: &FileDescriptor) -> HashAndFormat {
    HashAndFormat {
        hash: descriptor.hash.into(),
        format: BlobFormat::Raw,
    }
}

fn read_file_chunk(
    source_path: &Path,
    file: &mut fs::File,
    descriptor: &FileDescriptor,
    index: u64,
) -> Result<Vec<u8>> {
    let offset = index * u64::from(descriptor.chunk_size_bytes);
    file.seek(SeekFrom::Start(offset))
        .with_context(|| format!("seek source file at {} to {offset}", source_path.display()))?;
    let expected_len = expected_chunk_len(descriptor, index)?;
    let mut buf = vec![0u8; expected_len];
    file.read_exact(&mut buf)
        .with_context(|| format!("read source file chunk from {}", source_path.display()))?;
    Ok(buf)
}

fn expected_chunk_len(descriptor: &FileDescriptor, index: u64) -> Result<usize> {
    let chunk_count = chunk_count(descriptor);
    ensure!(index < chunk_count, "chunk index out of bounds");
    let chunk_size = u64::from(descriptor.chunk_size_bytes);
    let offset = index * chunk_size;
    Ok(descriptor.size_bytes.saturating_sub(offset).min(chunk_size) as usize)
}

fn chunk_count(descriptor: &FileDescriptor) -> u64 {
    if descriptor.size_bytes == 0 {
        0
    } else {
        descriptor
            .size_bytes
            .div_ceil(u64::from(descriptor.chunk_size_bytes))
    }
}

async fn read_exact_or_eof(recv: &mut iroh::endpoint::RecvStream, buf: &mut [u8]) -> Result<bool> {
    let mut offset = 0;
    while offset < buf.len() {
        match recv
            .read(&mut buf[offset..])
            .await
            .context("read stream bytes")?
        {
            Some(read) => offset += read,
            None if offset == 0 => return Ok(false),
            None => bail!("stream ended mid-frame"),
        }
    }
    Ok(true)
}

async fn read_exact_or_error(recv: &mut iroh::endpoint::RecvStream, buf: &mut [u8]) -> Result<()> {
    if !read_exact_or_eof(recv, buf).await? {
        bail!("stream ended before payload completed");
    }
    Ok(())
}

fn exchange_pairing(
    code: ShortCode,
    now: SystemTime,
    ttl: Duration,
) -> Result<(crate::EstablishedPairing, crate::EstablishedPairing)> {
    let mut left = PairingHandshake::new(code.clone(), now, ttl);
    let mut right = PairingHandshake::new(code, now, ttl);
    let left_pake = left.outbound_pake_message().to_vec();
    let right_pake = right.outbound_pake_message().to_vec();
    let left_pairing = left.finish(&right_pake, now)?.clone();
    let right_pairing = right.finish(&left_pake, now)?.clone();
    Ok((left_pairing, right_pairing))
}

fn unix_secs(value: SystemTime) -> u64 {
    value
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time is after unix epoch")
        .as_secs()
}

fn make_temp_dir(prefix: &str) -> Result<PathBuf> {
    let mut random = [0u8; 8];
    OsRng.fill_bytes(&mut random);
    let suffix = u64::from_be_bytes(random);
    let path = std::env::temp_dir().join(format!("altair-vega-{prefix}-{suffix:016x}"));
    fs::create_dir_all(&path).with_context(|| format!("create temp dir at {}", path.display()))?;
    Ok(path)
}

fn hash_to_hex(hash: [u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        FileProbeMode, FileProbeRunOptions, choose_transport, fetch_native_blob,
        run_local_file_probe, run_local_file_probe_with_options, spawn_native_blob_provider,
        verify_native_store,
    };
    use crate::{FileProgressPhase, FileTransport, MessagingPeerKind, ShortCode};
    use anyhow::Context;
    use iroh_blobs::protocol::{ChunkRanges, ChunkRangesExt, GetRequest};
    use std::{fs, str::FromStr};
    use tempfile::TempDir;

    #[test]
    fn chooses_native_blob_only_for_cli_pairs() {
        assert_eq!(
            choose_transport(MessagingPeerKind::Cli, MessagingPeerKind::Cli),
            FileTransport::NativeBlob
        );
        assert_eq!(
            choose_transport(MessagingPeerKind::Cli, MessagingPeerKind::Web),
            FileTransport::ChunkedStream
        );
        assert_eq!(
            choose_transport(MessagingPeerKind::Web, MessagingPeerKind::Web),
            FileTransport::ChunkedStream
        );
    }

    #[tokio::test]
    async fn transfers_file_successfully() {
        let code = ShortCode::from_str("2048-badar-celen-votun").unwrap();
        let outcome = run_local_file_probe(
            code.clone(),
            MessagingPeerKind::Cli,
            MessagingPeerKind::Cli,
            "demo.txt",
            b"hello from native blob path",
            FileProbeMode::Accept,
        )
        .await
        .unwrap();

        assert_eq!(outcome.code, code);
        assert!(outcome.accepted);
        assert!(!outcome.cancelled);
        assert_eq!(outcome.transport, FileTransport::NativeBlob);
        assert_eq!(outcome.received_hash, Some(outcome.expected_hash));
        assert!(outcome.resumed_local_bytes <= outcome.bytes_received);
        assert!(
            outcome
                .sender_progress
                .iter()
                .any(|item| item.phase == FileProgressPhase::Completed)
        );
        assert!(
            outcome
                .receiver_progress
                .iter()
                .any(|item| item.phase == FileProgressPhase::Completed)
        );
    }

    #[tokio::test]
    async fn rejects_file_transfer_cleanly() {
        let outcome = run_local_file_probe(
            ShortCode::from_str("2048-badar-celen-votun").unwrap(),
            MessagingPeerKind::Cli,
            MessagingPeerKind::Web,
            "reject.txt",
            b"receiver says no",
            FileProbeMode::Reject,
        )
        .await
        .unwrap();

        assert!(!outcome.accepted);
        assert!(outcome.cancelled);
        assert_eq!(outcome.bytes_sent, 0);
        assert_eq!(outcome.bytes_received, 0);
    }

    #[tokio::test]
    async fn can_retry_after_a_rejection() {
        let code = ShortCode::from_str("2048-badar-celen-votun").unwrap();
        let root = TempDir::new().unwrap();
        let state_root = root.path().join("receiver");

        let first = run_local_file_probe_with_options(
            code.clone(),
            MessagingPeerKind::Cli,
            MessagingPeerKind::Web,
            "retry.txt",
            b"first attempt",
            FileProbeRunOptions {
                mode: FileProbeMode::Reject,
                receiver_state_root: Some(state_root.clone()),
                interrupt_after_chunks: None,
            },
        )
        .await
        .unwrap();
        assert!(first.cancelled);

        let second = run_local_file_probe_with_options(
            code,
            MessagingPeerKind::Cli,
            MessagingPeerKind::Web,
            "retry.txt",
            b"second attempt",
            FileProbeRunOptions {
                mode: FileProbeMode::Accept,
                receiver_state_root: Some(state_root),
                interrupt_after_chunks: None,
            },
        )
        .await
        .unwrap();
        assert!(second.accepted);
        assert_eq!(second.transport, FileTransport::ChunkedStream);
        assert_eq!(second.received_hash, Some(second.expected_hash));
    }

    #[tokio::test]
    async fn detects_hash_mismatch() {
        let outcome = run_local_file_probe(
            ShortCode::from_str("2048-badar-celen-votun").unwrap(),
            MessagingPeerKind::Cli,
            MessagingPeerKind::Web,
            "tampered.txt",
            b"hash mismatch payload",
            FileProbeMode::HashMismatch,
        )
        .await
        .unwrap();

        assert!(!outcome.accepted);
        assert!(outcome.cancelled);
        assert_ne!(outcome.received_hash, Some(outcome.expected_hash));
        assert_eq!(outcome.reason.as_deref(), Some("file hash mismatch"));
    }

    #[tokio::test]
    async fn native_resume_survives_store_restart() {
        let root = TempDir::new().unwrap();
        let source_path = root.path().join("source.bin");
        let payload = vec![0x5a; (super::DEFAULT_CHUNK_SIZE_BYTES as usize * 6) + 12_345];
        fs::write(&source_path, &payload).unwrap();

        let provider = spawn_native_blob_provider(&source_path, &root.path().join("provider"))
            .await
            .unwrap();
        let receiver_store = root.path().join("receiver-store");

        let store = super::FsStore::load(&receiver_store)
            .await
            .context("load native receiver store for partial seed")
            .unwrap();
        let endpoint = super::Endpoint::bind(super::presets::N0)
            .await
            .context("bind native seed endpoint")
            .unwrap();
        let connection = endpoint
            .connect(provider.ticket.addr().clone(), iroh_blobs::ALPN)
            .await
            .context("connect for partial native seed")
            .unwrap();
        let request = GetRequest::blob_ranges(provider.ticket.hash(), ChunkRanges::chunks(..2u64));
        store
            .remote()
            .execute_get(connection, request)
            .await
            .context("download partial native ranges")
            .unwrap();
        endpoint.close().await;
        store.shutdown().await.unwrap();

        let descriptor = super::FileDescriptor {
            name: "resume.bin".to_string(),
            size_bytes: payload.len() as u64,
            hash: *blake3::hash(&payload).as_bytes(),
            chunk_size_bytes: super::DEFAULT_CHUNK_SIZE_BYTES,
        };

        let (before_bytes, before_complete) =
            super::inspect_native_store(&receiver_store, &descriptor)
                .await
                .unwrap();
        assert!(before_bytes > 0);
        assert!(before_bytes < descriptor.size_bytes);
        assert!(!before_complete);

        let resumed = fetch_native_blob(&receiver_store, &provider.ticket)
            .await
            .unwrap();
        assert_eq!(resumed.bytes_received, descriptor.size_bytes);
        assert_eq!(resumed.received_hash, descriptor.hash);

        let verified = verify_native_store(&receiver_store, &descriptor)
            .await
            .unwrap();
        assert_eq!(verified.0, descriptor.size_bytes);
        assert_eq!(verified.1, descriptor.hash);

        provider.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn chunked_resume_reuses_persisted_state_after_interrupt() {
        let code = ShortCode::from_str("2048-badar-celen-votun").unwrap();
        let root = TempDir::new().unwrap();
        let state_root = root.path().join("chunked-state");
        let payload = vec![0x33; (super::DEFAULT_CHUNK_SIZE_BYTES as usize * 5) + 8_192];

        let interrupted = run_local_file_probe_with_options(
            code.clone(),
            MessagingPeerKind::Cli,
            MessagingPeerKind::Web,
            "resume-web.bin",
            &payload,
            FileProbeRunOptions {
                mode: FileProbeMode::Accept,
                receiver_state_root: Some(state_root.clone()),
                interrupt_after_chunks: Some(2),
            },
        )
        .await
        .unwrap();
        assert!(interrupted.cancelled);
        assert!(interrupted.bytes_received > 0);
        assert!(interrupted.bytes_received < payload.len() as u64);

        let resumed = run_local_file_probe_with_options(
            code,
            MessagingPeerKind::Cli,
            MessagingPeerKind::Web,
            "resume-web.bin",
            &payload,
            FileProbeRunOptions {
                mode: FileProbeMode::Accept,
                receiver_state_root: Some(state_root),
                interrupt_after_chunks: None,
            },
        )
        .await
        .unwrap();
        assert!(resumed.accepted);
        assert!(resumed.resumed_local_bytes >= interrupted.bytes_received);
        assert!(resumed.bytes_sent < payload.len() as u64);
        assert_eq!(resumed.received_hash, Some(resumed.expected_hash));
    }

    #[tokio::test]
    async fn changed_content_starts_a_new_transfer() {
        let code = ShortCode::from_str("2048-badar-celen-votun").unwrap();
        let root = TempDir::new().unwrap();
        let state_root = root.path().join("changed-state");

        let first = run_local_file_probe_with_options(
            code.clone(),
            MessagingPeerKind::Web,
            MessagingPeerKind::Web,
            "same-name.bin",
            b"first version",
            FileProbeRunOptions {
                mode: FileProbeMode::Accept,
                receiver_state_root: Some(state_root.clone()),
                interrupt_after_chunks: None,
            },
        )
        .await
        .unwrap();
        assert!(first.accepted);

        let second = run_local_file_probe_with_options(
            code,
            MessagingPeerKind::Web,
            MessagingPeerKind::Web,
            "same-name.bin",
            b"second version with changed content",
            FileProbeRunOptions {
                mode: FileProbeMode::Accept,
                receiver_state_root: Some(state_root),
                interrupt_after_chunks: None,
            },
        )
        .await
        .unwrap();
        assert!(second.accepted);
        assert_eq!(second.resumed_local_bytes, 0);
    }
}
