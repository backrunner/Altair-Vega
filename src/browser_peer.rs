use altair_vega::FileDescriptor;
use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use iroh::{
    Endpoint,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler, Router},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};
use tokio::{fs, io::AsyncWriteExt};
use tokio_tungstenite::connect_async;
use url::Url;

pub const BROWSER_MESSAGE_ALPN: &[u8] = b"altair-vega/browser-message/1";
pub const BROWSER_FILE_ALPN: &[u8] = b"altair-vega/browser-file/1";
const MAX_MESSAGE_BYTES: usize = 256 * 1024;
const FILE_CHUNK_HEADER_BYTES: usize = 8 + 4 + 32;

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
struct BrowserPeerMessageHandler;

#[derive(Debug, Clone)]
struct BrowserPeerFileHandler {
    output_dir: PathBuf,
}

pub async fn run_browser_peer(
    code: String,
    room_url: String,
    output_dir: PathBuf,
) -> Result<()> {
    fs::create_dir_all(&output_dir)
        .await
        .with_context(|| format!("create browser peer output dir at {}", output_dir.display()))?;

    let endpoint = Endpoint::builder(iroh::endpoint::presets::N0)
        .alpns(vec![BROWSER_MESSAGE_ALPN.to_vec(), BROWSER_FILE_ALPN.to_vec()])
        .bind()
        .await
        .context("bind native browser-peer endpoint")?;

    let endpoint_id = endpoint.id().to_string();
    let router = Router::builder(endpoint)
        .accept(BROWSER_MESSAGE_ALPN, BrowserPeerMessageHandler)
        .accept(
            BROWSER_FILE_ALPN,
            BrowserPeerFileHandler {
                output_dir: output_dir.clone(),
            },
        )
        .spawn();

    let mut url = Url::parse(&room_url).context("parse browser peer room URL")?;
    url.query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("endpointId", &endpoint_id)
        .append_pair("peerType", "native-browser-peer")
        .append_pair("label", "Native Browser Peer");

    let (ws, _) = connect_async(url.as_str())
        .await
        .with_context(|| format!("connect native peer to room service at {url}"))?;

    println!("browser peer online");
    println!("code: {code}");
    println!("endpoint: {endpoint_id}");
    println!("output dir: {}", output_dir.display());
    println!("press Ctrl+C to stop");

    let (_write, mut read) = ws.split();

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = async {
            while let Some(message) = read.next().await {
                let _ = message?;
            }
            Ok::<(), tokio_tungstenite::tungstenite::Error>(())
        } => {}
    }

    router.shutdown().await.context("shutdown browser peer router")?;
    Ok(())
}

impl ProtocolHandler for BrowserPeerMessageHandler {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let remote_id = connection.remote_id().to_string();
        let (mut send, mut recv) = connection.accept_bi().await?;
        let bytes = recv.read_to_end(MAX_MESSAGE_BYTES).await.map_err(map_accept_error)?;
        let packet: BrowserPacket = serde_json::from_slice(&bytes).map_err(map_accept_error)?;
        println!("browser message from {remote_id}: {}", packet.body);
        send.write_all(b"ok").await.map_err(map_accept_error)?;
        send.finish()?;
        connection.closed().await;
        Ok(())
    }
}

impl ProtocolHandler for BrowserPeerFileHandler {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let remote_id = connection.remote_id().to_string();
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

        let mut chunks = BTreeMap::<u64, Vec<u8>>::new();
        let mut total_bytes = 0u64;
        while let Some(chunk_index) = read_chunk_index(&mut recv).await.map_err(map_accept_error)? {
            let mut chunk_header_rest = [0u8; FILE_CHUNK_HEADER_BYTES - 8];
            read_exact_or_error(&mut recv, &mut chunk_header_rest)
                .await
                .map_err(map_accept_error)?;
            let chunk_len = u32::from_be_bytes(
                chunk_header_rest[..4]
                    .try_into()
                    .expect("chunk len header bytes are correct"),
            ) as usize;
            let expected_hash: [u8; 32] = chunk_header_rest[4..]
                .try_into()
                .expect("chunk hash header bytes are correct");
            let mut chunk = vec![0u8; chunk_len];
            read_exact_or_error(&mut recv, &mut chunk)
                .await
                .map_err(map_accept_error)?;
            let actual_hash = *blake3::hash(&chunk).as_bytes();
            if actual_hash != expected_hash {
                return Err(map_accept_error("browser/native file chunk hash mismatch"));
            }
            total_bytes += chunk.len() as u64;
            chunks.insert(chunk_index, chunk);
        }

        let output_path = unique_output_path(&self.output_dir, &header.descriptor.name);
        let bytes = assemble_file_bytes(&header.descriptor, chunks).map_err(map_accept_error)?;
        let actual_hash = *blake3::hash(&bytes).as_bytes();
        if actual_hash != header.descriptor.hash {
            return Err(map_accept_error("browser/native file hash mismatch"));
        }
        let mut file = fs::File::create(&output_path)
            .await
            .with_context(|| format!("create browser peer file at {}", output_path.display()))
            .map_err(map_accept_error)?;
        file.write_all(&bytes)
            .await
            .with_context(|| format!("write browser peer file at {}", output_path.display()))
            .map_err(map_accept_error)?;
        file.flush().await.map_err(map_accept_error)?;

        println!(
            "browser file from {remote_id}: {} ({} bytes) saved to {}",
            header.descriptor.name,
            total_bytes,
            output_path.display()
        );

        send.write_all(b"ok").await.map_err(map_accept_error)?;
        send.finish()?;
        connection.closed().await;
        Ok(())
    }
}

async fn read_chunk_index(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<Option<u64>> {
    let mut chunk_index_buf = [0u8; 8];
    if !read_exact_or_eof(recv, &mut chunk_index_buf).await? {
        return Ok(None);
    }
    Ok(Some(u64::from_be_bytes(chunk_index_buf)))
}

async fn read_exact_or_eof(
    recv: &mut iroh::endpoint::RecvStream,
    buf: &mut [u8],
) -> Result<bool> {
    let mut offset = 0;
    while offset < buf.len() {
        match recv.read(&mut buf[offset..]).await.context("read browser peer stream bytes")? {
            Some(read) => offset += read,
            None if offset == 0 => return Ok(false),
            None => bail!("browser peer stream ended mid-frame"),
        }
    }
    Ok(true)
}

async fn read_exact_or_error(recv: &mut iroh::endpoint::RecvStream, buf: &mut [u8]) -> Result<()> {
    if !read_exact_or_eof(recv, buf).await? {
        bail!("browser peer stream ended before expected payload completed");
    }
    Ok(())
}

fn assemble_file_bytes(
    descriptor: &FileDescriptor,
    chunks: BTreeMap<u64, Vec<u8>>,
) -> Result<Vec<u8>> {
    let total_chunks = chunk_count(descriptor);
    ensure!(chunks.len() as u64 == total_chunks, "native peer did not receive all file chunks");
    let mut bytes = Vec::with_capacity(descriptor.size_bytes as usize);
    for index in 0..total_chunks {
        let chunk = chunks
            .get(&index)
            .ok_or_else(|| anyhow::anyhow!("missing file chunk {index}"))?;
        bytes.extend_from_slice(chunk);
    }
    ensure!(bytes.len() as u64 == descriptor.size_bytes, "native peer file size mismatch");
    Ok(bytes)
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

fn unique_output_path(output_dir: &Path, name: &str) -> PathBuf {
    let sanitized = name.replace(['/', '\\'], "_");
    let mut path = output_dir.join(&sanitized);
    if !path.exists() {
        return path;
    }

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_millis();
    let stem = Path::new(&sanitized)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("received");
    let ext = Path::new(&sanitized)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let file_name = if ext.is_empty() {
        format!("{stem}-{millis}")
    } else {
        format!("{stem}-{millis}.{ext}")
    };
    path = output_dir.join(file_name);
    path
}

fn map_accept_error(err: impl std::fmt::Display) -> AcceptError {
    std::io::Error::other(err.to_string()).into()
}
