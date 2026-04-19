use crate::{CURRENT_PROTOCOL_VERSION, EstablishedPairing, IrohBootstrapBundle, PeerCapabilities};
use anyhow::{Context, Result, bail, ensure};
use iroh::{
    Endpoint,
    endpoint::{Connection, ConnectionError, ReadError},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const CONTROL_ALPN: &[u8] = b"altair-vega/control/1";
const MAX_CONTROL_FRAME_LEN: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessagingPeerKind {
    Cli,
    Web,
}

impl MessagingPeerKind {
    pub const fn capabilities(self) -> PeerCapabilities {
        match self {
            Self::Cli => PeerCapabilities::cli(),
            Self::Web => PeerCapabilities::web(),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Cli => "cli-demo",
            Self::Web => "web-demo",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: u64,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlBind {
    pub protocol_version: u16,
    pub session_tag: [u8; 16],
    pub peer_capabilities: PeerCapabilities,
    pub device_label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlFrame {
    Bind(ControlBind),
    Message(ChatMessage),
    Close { reason: String },
}

pub struct ControlSession {
    connection: Connection,
    send: iroh::endpoint::SendStream,
    recv: iroh::endpoint::RecvStream,
    next_message_id: u64,
    seen_message_ids: HashSet<u64>,
    remote_capabilities: PeerCapabilities,
    remote_device_label: Option<String>,
}

impl ControlSession {
    pub async fn connect(
        endpoint: &Endpoint,
        pairing: &EstablishedPairing,
        local_bundle: &IrohBootstrapBundle,
        remote_bundle: &IrohBootstrapBundle,
    ) -> Result<Self> {
        let connection = endpoint
            .connect(
                remote_bundle.endpoint_ticket.endpoint_addr().clone(),
                CONTROL_ALPN,
            )
            .await
            .context("dial control connection")?;
        let (send, recv) = connection.open_bi().await.context("open control stream")?;
        let mut session = Self::new(connection, send, recv, local_bundle);
        session
            .write_bind(pairing, local_bundle)
            .await
            .context("send bind frame")?;
        session
            .read_and_verify_bind(pairing, remote_bundle)
            .await
            .context("verify remote bind frame")?;
        Ok(session)
    }

    pub async fn accept(
        connection: Connection,
        pairing: &EstablishedPairing,
        local_bundle: &IrohBootstrapBundle,
        remote_bundle: &IrohBootstrapBundle,
    ) -> Result<Self> {
        let (send, recv) = connection
            .accept_bi()
            .await
            .context("accept control stream")?;
        let mut session = Self::new(connection, send, recv, local_bundle);
        session
            .read_and_verify_bind(pairing, remote_bundle)
            .await
            .context("verify remote bind frame")?;
        session
            .write_bind(pairing, local_bundle)
            .await
            .context("send bind frame")?;
        Ok(session)
    }

    pub fn remote_capabilities(&self) -> PeerCapabilities {
        self.remote_capabilities
    }

    pub fn remote_device_label(&self) -> Option<&str> {
        self.remote_device_label.as_deref()
    }

    pub fn remote_endpoint_id(&self) -> iroh::EndpointId {
        self.connection.remote_id()
    }

    pub async fn send_message(&mut self, body: impl Into<String>) -> Result<ChatMessage> {
        let message = ChatMessage {
            id: self.next_message_id,
            body: body.into(),
        };
        self.next_message_id += 1;
        self.send_message_with_id(message.id, message.body.clone())
            .await
            .context("send message with generated id")
    }

    pub async fn send_message_with_id(&mut self, id: u64, body: String) -> Result<ChatMessage> {
        let message = ChatMessage { id, body };
        write_frame(&mut self.send, &ControlFrame::Message(message.clone()))
            .await
            .context("write message frame")?;
        Ok(message)
    }

    pub async fn receive_message(&mut self) -> Result<Option<ChatMessage>> {
        loop {
            match read_frame(&mut self.recv)
                .await
                .context("read control frame")?
            {
                Some(ControlFrame::Message(message)) => {
                    if self.seen_message_ids.insert(message.id) {
                        return Ok(Some(message));
                    }
                }
                Some(ControlFrame::Close { .. }) => return Ok(None),
                Some(ControlFrame::Bind(_)) => {
                    bail!("received unexpected bind frame after session setup")
                }
                None => return Ok(None),
            }
        }
    }

    pub fn finish_sending(&mut self) -> Result<()> {
        self.send.finish().context("finish control send stream")?;
        Ok(())
    }

    pub async fn wait_for_send_completion(&self) -> Result<()> {
        let _ = self
            .send
            .stopped()
            .await
            .context("wait for peer to read control stream")?;
        Ok(())
    }

    fn new(
        connection: Connection,
        send: iroh::endpoint::SendStream,
        recv: iroh::endpoint::RecvStream,
        local_bundle: &IrohBootstrapBundle,
    ) -> Self {
        Self {
            connection,
            send,
            recv,
            next_message_id: u64::from_be_bytes(
                local_bundle.session_nonce[..8]
                    .try_into()
                    .expect("nonce length is correct"),
            ),
            seen_message_ids: HashSet::new(),
            remote_capabilities: PeerCapabilities::new(false, false, false),
            remote_device_label: None,
        }
    }

    async fn write_bind(
        &mut self,
        pairing: &EstablishedPairing,
        local_bundle: &IrohBootstrapBundle,
    ) -> Result<()> {
        let frame = ControlFrame::Bind(ControlBind {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            session_tag: pairing.connection_binding_tag(local_bundle),
            peer_capabilities: local_bundle.capabilities,
            device_label: local_bundle.device_label.clone(),
        });
        write_frame(&mut self.send, &frame).await
    }

    async fn read_and_verify_bind(
        &mut self,
        pairing: &EstablishedPairing,
        remote_bundle: &IrohBootstrapBundle,
    ) -> Result<()> {
        let Some(frame) = read_frame(&mut self.recv)
            .await
            .context("read bind frame")?
        else {
            bail!("peer closed control stream before sending bind frame");
        };

        let ControlFrame::Bind(bind) = frame else {
            bail!("expected bind frame as first control message");
        };

        ensure!(
            bind.protocol_version == CURRENT_PROTOCOL_VERSION,
            "remote control protocol version mismatch"
        );
        ensure!(
            bind.session_tag == pairing.connection_binding_tag(remote_bundle),
            "remote control bind tag mismatch"
        );
        ensure!(
            bind.peer_capabilities == remote_bundle.capabilities,
            "remote control capabilities do not match bootstrap bundle"
        );
        ensure!(
            bind.device_label == remote_bundle.device_label,
            "remote control device label does not match bootstrap bundle"
        );

        self.remote_capabilities = bind.peer_capabilities;
        self.remote_device_label = bind.device_label;
        Ok(())
    }
}

pub fn encode_frame(frame: &ControlFrame) -> Result<Vec<u8>> {
    serde_json::to_vec(frame).context("serialize control frame")
}

pub fn decode_frame(bytes: &[u8]) -> Result<ControlFrame> {
    serde_json::from_slice(bytes).context("deserialize control frame")
}

async fn write_frame(send: &mut iroh::endpoint::SendStream, frame: &ControlFrame) -> Result<()> {
    let payload = encode_frame(frame)?;
    ensure!(
        payload.len() <= MAX_CONTROL_FRAME_LEN,
        "control frame exceeds max size"
    );
    let len = u32::try_from(payload.len()).context("frame length overflow")?;
    send.write_all(&len.to_be_bytes())
        .await
        .context("write control frame length")?;
    send.write_all(&payload)
        .await
        .context("write control frame payload")?;
    Ok(())
}

async fn read_frame(recv: &mut iroh::endpoint::RecvStream) -> Result<Option<ControlFrame>> {
    let mut len_buf = [0u8; 4];
    if !read_exact_or_eof(recv, &mut len_buf)
        .await
        .context("read control frame length")?
    {
        return Ok(None);
    }

    let len = u32::from_be_bytes(len_buf) as usize;
    ensure!(
        len <= MAX_CONTROL_FRAME_LEN,
        "control frame exceeds max size"
    );
    let mut payload = vec![0u8; len];
    recv.read_exact(&mut payload)
        .await
        .context("read control frame payload")?;
    Ok(Some(decode_frame(&payload)?))
}

async fn read_exact_or_eof(recv: &mut iroh::endpoint::RecvStream, buf: &mut [u8]) -> Result<bool> {
    let mut offset = 0;
    while offset < buf.len() {
        match recv.read(&mut buf[offset..]).await {
            Ok(Some(read)) => offset += read,
            Ok(None) if offset == 0 => return Ok(false),
            Ok(None) => bail!("control stream ended mid-frame"),
            Err(ReadError::ConnectionLost(ConnectionError::ApplicationClosed(_)))
                if offset == 0 =>
            {
                return Ok(false);
            }
            Err(ReadError::ConnectionLost(ConnectionError::LocallyClosed)) if offset == 0 => {
                return Ok(false);
            }
            Err(error) => return Err(error).context("read control stream bytes"),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{
        ChatMessage, ControlBind, ControlFrame, MessagingPeerKind, decode_frame, encode_frame,
    };
    use crate::{CURRENT_PROTOCOL_VERSION, PeerCapabilities};

    #[test]
    fn control_frame_round_trips_through_json() {
        let frame = ControlFrame::Message(ChatMessage {
            id: 7,
            body: "hello".to_string(),
        });
        let encoded = encode_frame(&frame).unwrap();
        let decoded = decode_frame(&encoded).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn bind_frame_round_trips_through_json() {
        let frame = ControlFrame::Bind(ControlBind {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            session_tag: [9u8; 16],
            peer_capabilities: PeerCapabilities::web(),
            device_label: Some("web-demo".to_string()),
        });
        let encoded = encode_frame(&frame).unwrap();
        let decoded = decode_frame(&encoded).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn peer_kind_maps_to_expected_capabilities() {
        assert_eq!(
            MessagingPeerKind::Cli.capabilities(),
            PeerCapabilities::cli()
        );
        assert_eq!(
            MessagingPeerKind::Web.capabilities(),
            PeerCapabilities::web()
        );
    }
}
