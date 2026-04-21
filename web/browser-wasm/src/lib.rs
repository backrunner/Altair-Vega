use altair_vega::{FileChunkRange, FileDescriptor, ShortCode};
use anyhow::{Context, Result};
use async_channel::Sender;
use iroh::{
    Endpoint, EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler, Router},
};
use n0_future::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tracing::level_filters::LevelFilter;
use tracing_subscriber_wasm::MakeConsoleWriter;
use wasm_bindgen::{JsError, prelude::wasm_bindgen};
use wasm_streams::{ReadableStream, readable::sys::ReadableStream as JsReadableStream};

const WEB_MESSAGE_ALPN: &[u8] = b"altair-vega/browser-message/1";
const WEB_FILE_ALPN: &[u8] = b"altair-vega/browser-file/1";
const MAX_MESSAGE_BYTES: usize = 256 * 1024;
const FILE_CHUNK_HEADER_BYTES: usize = 8 + 4 + 32;

#[derive(Debug, Clone)]
pub struct BrowserNode {
    router: Router,
    shared: Arc<BrowserSharedState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BrowserEvent {
    Ready { endpoint_id: String },
    ReceivedMessage { endpoint_id: String, body: String },
    SentMessage { endpoint_id: String, body: String },
    ReceivedFile {
        endpoint_id: String,
        transfer_id: u64,
        name: String,
        size_bytes: u64,
        hash_hex: String,
        mime_type: String,
    },
    SentFile {
        endpoint_id: String,
        transfer_id: u64,
        name: String,
        size_bytes: u64,
    },
    ReceivedFileChunk {
        endpoint_id: String,
        transfer_id: u64,
        chunk_index: u64,
        name: String,
        size_bytes: u64,
        chunk_size_bytes: u32,
        chunk_bytes: u32,
        bytes_complete: u64,
        hash_hex: String,
        mime_type: String,
    },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BrowserPacket {
    body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BrowserFileHeader {
    transfer_id: u64,
    descriptor: FileDescriptor,
    mime_type: String,
}

#[derive(Debug, Clone)]
struct BrowserReceivedFile {
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct BrowserSharedState {
    event_sender: Sender<BrowserEvent>,
    next_transfer_id: AtomicU64,
    received_files: Mutex<HashMap<u64, BrowserReceivedFile>>,
    received_chunks: Mutex<HashMap<(u64, u64), Vec<u8>>>,
}

#[derive(Debug, Clone)]
struct BrowserMessageProtocol {
    shared: Arc<BrowserSharedState>,
}

#[derive(Debug, Clone)]
struct BrowserFileProtocol {
    shared: Arc<BrowserSharedState>,
}

impl BrowserNode {
    async fn spawn_inner() -> Result<(Self, async_channel::Receiver<BrowserEvent>)> {
        let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
            .alpns(vec![WEB_MESSAGE_ALPN.to_vec(), WEB_FILE_ALPN.to_vec()])
            .bind()
            .await
            .context("bind browser endpoint")?;
        let (event_sender, event_receiver) = async_channel::unbounded();
        let shared = Arc::new(BrowserSharedState {
            event_sender: event_sender.clone(),
            next_transfer_id: AtomicU64::new(1),
            received_files: Mutex::new(HashMap::new()),
            received_chunks: Mutex::new(HashMap::new()),
        });
        event_sender
            .send(BrowserEvent::Ready {
                endpoint_id: endpoint.id().to_string(),
            })
            .await
            .ok();
        let message_protocol = BrowserMessageProtocol {
            shared: shared.clone(),
        };
        let file_protocol = BrowserFileProtocol {
            shared: shared.clone(),
        };
        let router = Router::builder(endpoint)
            .accept(WEB_MESSAGE_ALPN, message_protocol)
            .accept(WEB_FILE_ALPN, file_protocol)
            .spawn();
        Ok((Self { router, shared }, event_receiver))
    }

    async fn send_message_inner(&self, endpoint_id: EndpointId, body: String) -> Result<()> {
        let connection = self
            .router
            .endpoint()
            .connect(endpoint_id, WEB_MESSAGE_ALPN)
            .await
            .context("dial remote browser endpoint")?;

        let (mut send, mut recv) = connection.open_bi().await.context("open message stream")?;
        let payload = serde_json::to_vec(&BrowserPacket { body: body.clone() })
            .context("serialize browser packet")?;
        send.write_all(&payload)
            .await
            .context("write browser packet")?;
        send.finish().context("finish browser send stream")?;
        let _ = recv
            .read_to_end(MAX_MESSAGE_BYTES)
            .await
            .context("read browser ack")?;
        connection.close(0u8.into(), b"done");
        self.shared
            .event_sender
            .send(BrowserEvent::SentMessage {
                endpoint_id: endpoint_id.to_string(),
                body,
            })
            .await
            .ok();
        Ok(())
    }

    async fn send_file_inner(
        &self,
        endpoint_id: EndpointId,
        name: String,
        mime_type: String,
        bytes: Vec<u8>,
        missing_ranges: Option<Vec<FileChunkRange>>,
    ) -> Result<()> {
        let transfer_id = self.shared.next_transfer_id.fetch_add(1, Ordering::Relaxed);
        let descriptor = FileDescriptor {
            name: name.clone(),
            size_bytes: bytes.len() as u64,
            hash: *blake3::hash(&bytes).as_bytes(),
            chunk_size_bytes: 256 * 1024,
        };
        let header = BrowserFileHeader {
            transfer_id,
            descriptor: descriptor.clone(),
            mime_type,
        };
        let header_bytes = serde_json::to_vec(&header).context("serialize browser file header")?;
        let header_len = u32::try_from(header_bytes.len()).context("browser file header too large")?;
        let connection = self
            .router
            .endpoint()
            .connect(endpoint_id, WEB_FILE_ALPN)
            .await
            .context("dial remote browser file endpoint")?;
        let (mut send, mut recv) = connection.open_bi().await.context("open file stream")?;
        send.write_all(&header_len.to_be_bytes())
            .await
            .context("write file header length")?;
        send.write_all(&header_bytes)
            .await
            .context("write file header")?;

        let ranges = missing_ranges.unwrap_or_else(|| vec![FileChunkRange {
            start: 0,
            end: chunk_count(&descriptor),
        }]);

        for range in ranges {
            for chunk_index in range.start..range.end {
                let (chunk, chunk_hash) = read_chunk(&bytes, &descriptor, chunk_index)?;
                let chunk_len =
                    u32::try_from(chunk.len()).context("browser file chunk too large")?;
                send.write_all(&chunk_index.to_be_bytes())
                    .await
                    .context("write file chunk index")?;
                send.write_all(&chunk_len.to_be_bytes())
                    .await
                    .context("write file chunk length")?;
                send.write_all(&chunk_hash)
                    .await
                    .context("write file chunk hash")?;
                send.write_all(&chunk)
                    .await
                    .context("write file chunk payload")?;
            }
        }
        send.finish().context("finish browser file send stream")?;
        let _ = recv
            .read_to_end(1024)
            .await
            .context("read browser file ack")?;
        connection.close(0u8.into(), b"done");

        self.shared
            .event_sender
            .send(BrowserEvent::SentFile {
                endpoint_id: endpoint_id.to_string(),
                transfer_id,
                name,
                size_bytes: descriptor.size_bytes,
            })
            .await
            .ok();
        Ok(())
    }

    fn take_received_file_inner(&self, transfer_id: u64) -> Option<BrowserReceivedFile> {
        self.shared
            .received_files
            .lock()
            .expect("received file mutex should not be poisoned")
            .remove(&transfer_id)
    }

    fn endpoint_id_string(&self) -> String {
        self.router.endpoint().id().to_string()
    }
}

impl BrowserMessageProtocol {
    async fn handle_connection(self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let endpoint_id = connection.remote_id().to_string();
        let res = self.handle_connection_0(&connection).await;
        if let Err(error) = &res {
            self.shared
                .event_sender
                .send(BrowserEvent::Error {
                    message: format!("browser connection error from {endpoint_id}: {error}"),
                })
                .await
                .ok();
        }
        res
    }

    async fn handle_connection_0(&self, connection: &Connection) -> std::result::Result<(), AcceptError> {
        let endpoint_id = connection.remote_id().to_string();
        let (mut send, mut recv) = connection.accept_bi().await?;
        let bytes = recv
            .read_to_end(MAX_MESSAGE_BYTES)
            .await
            .map_err(map_accept_error)?;
        let packet: BrowserPacket = serde_json::from_slice(&bytes).map_err(map_accept_error)?;
        self.shared
            .event_sender
            .send(BrowserEvent::ReceivedMessage {
                endpoint_id,
                body: packet.body,
            })
            .await
            .ok();
        send.write_all(b"ok").await.map_err(map_accept_error)?;
        send.finish()?;
        connection.closed().await;
        Ok(())
    }
}

impl ProtocolHandler for BrowserMessageProtocol {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        self.clone().handle_connection(connection).await
    }
}

impl BrowserFileProtocol {
    async fn handle_connection(self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let endpoint_id = connection.remote_id().to_string();
        let res = self.handle_connection_0(&connection).await;
        if let Err(error) = &res {
            self.shared
                .event_sender
                .send(BrowserEvent::Error {
                    message: format!("browser file connection error from {endpoint_id}: {error}"),
                })
                .await
                .ok();
        }
        res
    }

    async fn handle_connection_0(
        &self,
        connection: &Connection,
    ) -> std::result::Result<(), AcceptError> {
        let endpoint_id = connection.remote_id().to_string();
        let (mut send, mut recv) = connection.accept_bi().await?;
        let mut header_len_buf = [0u8; 4];
        if !read_exact_or_eof(&mut recv, &mut header_len_buf)
            .await
            .map_err(map_accept_error)?
        {
            return Err(map_accept_error("missing browser file header length"));
        }

        let header_len = u32::from_be_bytes(header_len_buf) as usize;
        let mut header_bytes = vec![0u8; header_len];
        read_exact_or_error(&mut recv, &mut header_bytes)
            .await
            .map_err(map_accept_error)?;
        let header: BrowserFileHeader =
            serde_json::from_slice(&header_bytes).map_err(map_accept_error)?;

        let mut bytes_complete = 0u64;

        loop {
            let mut chunk_header = [0u8; FILE_CHUNK_HEADER_BYTES];
            let has_chunk = read_exact_or_eof(&mut recv, &mut chunk_header)
                .await
                .map_err(map_accept_error)?;
            if !has_chunk {
                break;
            }

            let chunk_index = u64::from_be_bytes(
                chunk_header[..8]
                    .try_into()
                    .expect("chunk index header length is correct"),
            );
            let chunk_len = u32::from_be_bytes(
                chunk_header[8..12]
                    .try_into()
                    .expect("chunk len header length is correct"),
            ) as usize;
            let chunk_hash: [u8; 32] = chunk_header[12..]
                .try_into()
                .expect("chunk hash header length is correct");

            let mut chunk = vec![0u8; chunk_len];
            read_exact_or_error(&mut recv, &mut chunk)
                .await
                .map_err(map_accept_error)?;
            let actual_chunk_hash = *blake3::hash(&chunk).as_bytes();
            if actual_chunk_hash != chunk_hash {
                return Err(map_accept_error("browser file chunk hash mismatch"));
            }

            bytes_complete += chunk.len() as u64;
            self.shared
                .received_chunks
                .lock()
                .expect("received chunk mutex should not be poisoned")
                .insert((header.transfer_id, chunk_index), chunk);

            self.shared
                .event_sender
                .send(BrowserEvent::ReceivedFileChunk {
                    endpoint_id: endpoint_id.clone(),
                    transfer_id: header.transfer_id,
                    chunk_index,
                    name: header.descriptor.name.clone(),
                    size_bytes: header.descriptor.size_bytes,
                    chunk_size_bytes: header.descriptor.chunk_size_bytes,
                    chunk_bytes: chunk_len as u32,
                    bytes_complete,
                    hash_hex: hex::encode(header.descriptor.hash),
                    mime_type: header.mime_type.clone(),
                })
                .await
                .ok();
        }

        send.write_all(b"ok").await.map_err(map_accept_error)?;
        send.finish()?;
        connection.closed().await;
        Ok(())
    }
}

impl ProtocolHandler for BrowserFileProtocol {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        self.clone().handle_connection(connection).await
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();

    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .with_writer(MakeConsoleWriter::default().map_trace_level_to(tracing::Level::DEBUG))
        .without_time()
        .with_ansi(false)
        .init();
}

#[wasm_bindgen]
pub struct WasmBrowserNode {
    inner: BrowserNode,
    events: async_channel::Receiver<BrowserEvent>,
}

#[wasm_bindgen]
impl WasmBrowserNode {
    pub async fn spawn() -> Result<WasmBrowserNode, JsError> {
        let (inner, events) = BrowserNode::spawn_inner().await.map_err(to_js_err)?;
        Ok(Self { inner, events })
    }

    pub fn endpoint_id(&self) -> String {
        self.inner.endpoint_id_string()
    }

    pub fn events(&self) -> JsReadableStream {
        into_js_readable_stream(self.events.clone())
    }

    pub async fn send_message(&self, endpoint_id: String, body: String) -> Result<(), JsError> {
        let endpoint_id = endpoint_id
            .parse()
            .context("parse endpoint id")
            .map_err(to_js_err)?;
        self.inner
            .send_message_inner(endpoint_id, body.clone())
            .await
            .map_err(to_js_err)?;
        Ok(())
    }

    pub async fn send_file(
        &self,
        endpoint_id: String,
        name: String,
        mime_type: String,
        bytes: js_sys::Uint8Array,
    ) -> Result<(), JsError> {
        self.send_file_with_ranges(endpoint_id, name, mime_type, bytes, wasm_bindgen::JsValue::NULL)
            .await
    }

    pub async fn send_file_with_ranges(
        &self,
        endpoint_id: String,
        name: String,
        mime_type: String,
        bytes: js_sys::Uint8Array,
        missing_ranges: wasm_bindgen::JsValue,
    ) -> Result<(), JsError> {
        let endpoint_id = endpoint_id
            .parse()
            .context("parse endpoint id")
            .map_err(to_js_err)?;
        let missing_ranges = if missing_ranges.is_null() || missing_ranges.is_undefined() {
            None
        } else {
            Some(
                serde_wasm_bindgen::from_value::<Vec<FileChunkRange>>(missing_ranges)
                    .map_err(to_js_err)?,
            )
        };
        self.inner
            .send_file_inner(endpoint_id, name, mime_type, bytes.to_vec(), missing_ranges)
            .await
            .map_err(to_js_err)?;
        Ok(())
    }

    pub fn take_received_file(&self, transfer_id: u64) -> Result<js_sys::Uint8Array, JsError> {
        let file = self
            .inner
            .take_received_file_inner(transfer_id)
            .ok_or_else(|| JsError::new("received file not found"))?;
        Ok(js_sys::Uint8Array::from(file.bytes.as_slice()))
    }

    pub fn take_received_chunk(
        &self,
        transfer_id: u64,
        chunk_index: u64,
    ) -> Result<js_sys::Uint8Array, JsError> {
        let chunk = self
            .inner
            .shared
            .received_chunks
            .lock()
            .expect("received chunk mutex should not be poisoned")
            .remove(&(transfer_id, chunk_index))
            .ok_or_else(|| JsError::new("received chunk not found"))?;
        Ok(js_sys::Uint8Array::from(chunk.as_slice()))
    }

    pub async fn shutdown(self) -> Result<(), JsError> {
        self.inner.router.shutdown().await.map_err(to_js_err)?;
        Ok(())
    }
}

#[wasm_bindgen]
pub fn generate_short_code() -> String {
    ShortCode::generate().to_string()
}

#[wasm_bindgen]
pub fn normalize_short_code(value: String) -> Result<String, JsError> {
    let code = value.parse::<ShortCode>().map_err(to_js_err)?;
    Ok(code.normalized())
}

#[wasm_bindgen]
pub fn hash_bytes_hex(bytes: js_sys::Uint8Array) -> String {
    hex::encode(blake3::hash(&bytes.to_vec()).as_bytes())
}

fn to_js_err(err: impl Into<anyhow::Error>) -> JsError {
    let err: anyhow::Error = err.into();
    JsError::new(&err.to_string())
}

fn map_accept_error(err: impl std::fmt::Display) -> AcceptError {
    std::io::Error::other(err.to_string()).into()
}

fn into_js_readable_stream<T>(stream: impl Stream<Item = T> + 'static) -> JsReadableStream
where
    T: Serialize,
{
    let stream = stream.map(|event| Ok(serde_wasm_bindgen::to_value(&event).unwrap()));
    ReadableStream::from_stream(stream).into_raw()
}

async fn read_exact_or_eof(
    recv: &mut iroh::endpoint::RecvStream,
    buf: &mut [u8],
) -> Result<bool> {
    let mut offset = 0;
    while offset < buf.len() {
        match recv.read(&mut buf[offset..]).await.context("read browser file stream bytes")? {
            Some(read) => offset += read,
            None if offset == 0 => return Ok(false),
            None => anyhow::bail!("browser file stream ended mid-frame"),
        }
    }
    Ok(true)
}

async fn read_exact_or_error(recv: &mut iroh::endpoint::RecvStream, buf: &mut [u8]) -> Result<()> {
    if !read_exact_or_eof(recv, buf).await? {
        anyhow::bail!("browser file stream ended before expected payload completed");
    }
    Ok(())
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

fn read_chunk(
    bytes: &[u8],
    descriptor: &FileDescriptor,
    chunk_index: u64,
) -> Result<(Vec<u8>, [u8; 32])> {
    let chunk_size = u64::from(descriptor.chunk_size_bytes);
    let start = usize::try_from(chunk_index * chunk_size).context("browser chunk start overflow")?;
    let end = usize::try_from(((chunk_index + 1) * chunk_size).min(descriptor.size_bytes))
        .context("browser chunk end overflow")?;
    if start > end || end > bytes.len() {
        anyhow::bail!("browser chunk range out of bounds");
    }
    let chunk = bytes[start..end].to_vec();
    let hash = *blake3::hash(&chunk).as_bytes();
    Ok((chunk, hash))
}
