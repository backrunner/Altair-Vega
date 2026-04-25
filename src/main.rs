use altair_vega::{
    CONTROL_ALPN, ChatMessage, ControlFrame, ControlSession, EstablishedPairing, FileDescriptor,
    FileOffer, FileProbeConfig, FileProbeMode, FileProgress, FileProgressPhase, FileResponse,
    FileTicket, FileTransport, IrohBootstrapBundle, MessagingPeerKind, PairingHandshake,
    PairingIntroEnvelope, PeerCapabilities, ShortCode, apply_merge_plan, diff_manifests,
    keep_runtime_requested, manifests_state_eq, merge_manifests, preferred_runtime_parent,
    resolve_runtime_state_dir, run_local_file_probe, run_local_file_probe_with_config,
    run_local_message_probe, run_local_native_resume_probe, run_local_pairing_probe,
    runtime_root_from_env, scan_directory,
};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose};
use clap::{Args, Parser, Subcommand, ValueEnum};
use futures_util::{SinkExt, StreamExt};
use iroh::{
    Endpoint,
    address_lookup::{DiscoveryEvent, MdnsAddressLookup, UserData},
    endpoint::presets,
    protocol::Router,
};
use iroh_blobs::{
    BlobFormat, BlobsProtocol, HashAndFormat, store::fs::FsStore, ticket::BlobTicket,
};
use iroh_tickets::endpoint::EndpointTicket;
use notify::{EventKind, RecursiveMode, Watcher};
use qrcode::{Color, QrCode, render::unicode};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{IsTerminal as _, Write as _},
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, SystemTime},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

mod browser_peer;
mod sync_docs;

const DEFAULT_RENDEZVOUS_URL: &str = match option_env!("ALTAIR_VEGA_DEFAULT_RENDEZVOUS") {
    Some(value) => value,
    None => "ws://127.0.0.1:5173/__altair_vega_rendezvous",
};

#[derive(Debug, Parser)]
#[command(name = "altair-vega")]
#[command(about = "Peer-to-peer transfer and folder sync over short pairing codes")]
#[command(disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Send text messages or files to a paired native peer")]
    Send {
        #[command(subcommand)]
        command: SendCommand,
    },
    #[command(about = "Receive text messages or files from a paired native peer")]
    Receive {
        #[command(subcommand)]
        command: ReceiveCommand,
    },
    #[command(about = "Create or join a short-code pairing room")]
    Pair {
        #[command(flatten)]
        args: PairArgs,
    },
    #[command(
        about = "Host, follow, or join native folder sync sessions",
        args_conflicts_with_subcommands = true
    )]
    Sync {
        #[command(subcommand)]
        command: Option<SyncCommand>,
        #[command(flatten)]
        args: SyncArgs,
    },
    #[command(about = "Run bridge services for browser and native peers")]
    Serve {
        #[command(subcommand)]
        command: ServeCommand,
    },
    #[command(about = "Inspect disposable runtime and support diagnostics")]
    Runtime {
        #[command(subcommand)]
        command: RuntimeCommand,
    },
    #[command(about = "Read the Altair Vega manual")]
    Help {
        #[arg(long)]
        no_pager: bool,
        #[arg(value_name = "TOPIC", num_args = 0.., trailing_var_arg = true)]
        topic: Vec<String>,
    },
    #[command(hide = true)]
    #[command(about = "Developer validation and diagnostic commands")]
    Dev {
        #[command(subcommand)]
        command: DevCommand,
    },
    #[command(hide = true)]
    #[command(about = "Generate or inspect short codes")]
    Code {
        #[command(subcommand)]
        command: CodeCommand,
    },
    #[command(hide = true)]
    #[command(about = "Run pairing validation probes")]
    Pairing {
        #[command(subcommand)]
        command: PairingCommand,
    },
    #[command(hide = true)]
    #[command(about = "Run message validation probes")]
    Message {
        #[command(subcommand)]
        command: MessageCommand,
    },
    #[command(hide = true)]
    #[command(about = "Run file-transfer validation probes")]
    File {
        #[command(subcommand)]
        command: FileCommand,
    },
    #[command(hide = true)]
    #[command(about = "Run native browser-peer bridge diagnostics")]
    BrowserPeer {
        #[command(subcommand)]
        command: BrowserPeerCommand,
    },
}

#[derive(Debug, Args)]
struct SyncArgs {
    #[arg(help = "Folder to publish or receive synced files")]
    folder: Option<PathBuf>,
    #[arg(
        value_name = "KEY",
        help = "Optional short code or docs ticket with --naked"
    )]
    key: Option<String>,
    #[arg(long, default_value = DEFAULT_RENDEZVOUS_URL, help = "Rendezvous room URL for short-code sync")]
    room_url: String,
    #[arg(long, value_enum, default_value_t = PairMode::Persistent, help = "Pairing lifetime")]
    pair_mode: PairMode,
    #[arg(
        long,
        help = "Use or expose a raw iroh-docs ticket instead of rendezvous"
    )]
    naked: bool,
    #[arg(
        long,
        help = "Publish local changes bidirectionally instead of read-only follow"
    )]
    join: bool,
    #[arg(
        long,
        help = "Render the printed code or ticket as a terminal QR code when hosting"
    )]
    qr: bool,
    #[arg(long, help = "Directory for sync state")]
    state_dir: Option<PathBuf>,
    #[arg(
        long,
        default_value_t = 1500,
        help = "Initial wait for remote docs state in milliseconds when following"
    )]
    wait_ms: u64,
    #[arg(
        long,
        default_value_t = 1000,
        help = "Watch/poll interval in milliseconds"
    )]
    interval_ms: u64,
}

#[derive(Debug, Subcommand)]
enum SendCommand {
    #[command(about = "Send one text message to a native receiver")]
    Text {
        message: String,
        #[arg(value_name = "CODE")]
        code: Option<String>,
        #[arg(long, default_value = DEFAULT_RENDEZVOUS_URL)]
        room_url: String,
        #[arg(long, value_enum, default_value_t = PairMode::OneOff)]
        pair_mode: PairMode,
        #[arg(long)]
        naked: bool,
        #[arg(long)]
        qr: bool,
    },
    #[command(about = "Send one file to a native receiver")]
    File {
        path: PathBuf,
        #[arg(value_name = "CODE")]
        code: Option<String>,
        #[arg(long, default_value = DEFAULT_RENDEZVOUS_URL)]
        room_url: String,
        #[arg(long, value_enum, default_value_t = PairMode::OneOff)]
        pair_mode: PairMode,
        #[arg(long)]
        naked: bool,
        #[arg(long)]
        qr: bool,
        #[arg(long)]
        state_dir: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum ReceiveCommand {
    #[command(about = "Receive one text message from a native sender")]
    Text {
        code: Option<String>,
        #[arg(long, default_value = DEFAULT_RENDEZVOUS_URL)]
        room_url: String,
        #[arg(long, value_enum, default_value_t = PairMode::OneOff)]
        pair_mode: PairMode,
        #[arg(long)]
        naked: bool,
        #[arg(long)]
        qr: bool,
    },
    #[command(about = "Receive one file from a native sender")]
    File {
        code: Option<String>,
        #[arg(long, default_value = "received-files")]
        output_dir: PathBuf,
        #[arg(long, default_value = DEFAULT_RENDEZVOUS_URL)]
        room_url: String,
        #[arg(long, value_enum, default_value_t = PairMode::OneOff)]
        pair_mode: PairMode,
        #[arg(long)]
        naked: bool,
        #[arg(long)]
        state_dir: Option<PathBuf>,
    },
}

#[derive(Debug, Args)]
struct PairArgs {
    #[arg(value_name = "CODE", num_args = 0.., trailing_var_arg = true)]
    code: Vec<String>,
    #[arg(long, default_value = DEFAULT_RENDEZVOUS_URL)]
    room_url: String,
    #[arg(long, value_enum)]
    mode: Option<PairMode>,
    #[arg(long)]
    naked: bool,
    #[arg(long)]
    qr: bool,
    #[arg(long)]
    inspect: bool,
}

#[derive(Debug, Subcommand)]
enum ServeCommand {
    #[command(about = "Join a browser rendezvous room as a native peer")]
    BrowserPeer {
        code: String,
        #[arg(long, default_value = DEFAULT_RENDEZVOUS_URL)]
        room_url: String,
        #[arg(long, default_value = "browser-peer-downloads")]
        output_dir: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum DevCommand {
    #[command(about = "Generate or inspect short codes")]
    Code {
        #[command(subcommand)]
        command: CodeCommand,
    },
    #[command(about = "Run pairing validation probes")]
    Pairing {
        #[command(subcommand)]
        command: PairingCommand,
    },
    #[command(about = "Run message validation probes")]
    Message {
        #[command(subcommand)]
        command: MessageCommand,
    },
    #[command(about = "Run file-transfer validation probes")]
    File {
        #[command(subcommand)]
        command: FileCommand,
    },
    #[command(about = "Run native browser-peer bridge diagnostics")]
    BrowserPeer {
        #[command(subcommand)]
        command: BrowserPeerCommand,
    },
}

#[derive(Debug, Subcommand)]
enum CodeCommand {
    #[command(about = "Generate a new short pairing code")]
    Generate,
    #[command(about = "Normalize and inspect a short pairing code")]
    Inspect { code: String },
}

#[derive(Debug, Subcommand)]
enum PairingCommand {
    #[command(about = "Run a local pairing bootstrap probe")]
    Demo { code: Option<String> },
}

#[derive(Debug, Subcommand)]
enum MessageCommand {
    #[command(about = "Run a local message exchange probe")]
    Demo {
        code: Option<String>,
        #[arg(long, value_enum, default_value_t = PeerKindArg::Cli)]
        left: PeerKindArg,
        #[arg(long, value_enum, default_value_t = PeerKindArg::Cli)]
        right: PeerKindArg,
        #[arg(long, default_value = "hello from left")]
        left_text: String,
        #[arg(long, default_value = "hello from right")]
        right_text: String,
    },
}

#[derive(Debug, Subcommand)]
enum FileCommand {
    #[command(about = "Run a local file-transfer probe")]
    Demo {
        code: Option<String>,
        #[arg(long, value_enum, default_value_t = PeerKindArg::Cli)]
        left: PeerKindArg,
        #[arg(long, value_enum, default_value_t = PeerKindArg::Cli)]
        right: PeerKindArg,
        #[arg(long, default_value = "demo.txt")]
        name: String,
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long, default_value = "hello from file demo")]
        text: String,
        #[arg(long)]
        receiver_state_root: Option<PathBuf>,
        #[arg(long)]
        interrupt_after_chunks: Option<u64>,
    },
    #[command(about = "Run a native file-transfer restart/resume probe")]
    NativeResumeDemo {
        code: Option<String>,
        #[arg(long, default_value = "resume.bin")]
        name: String,
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long, default_value = "hello from native resume demo")]
        text: String,
        #[arg(long, default_value_t = 2)]
        seeded_chunks: u64,
        #[arg(long)]
        receiver_state_root: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum BrowserPeerCommand {
    #[command(about = "Serve a native peer in a browser rendezvous room")]
    Serve {
        code: String,
        #[arg(long, default_value = DEFAULT_RENDEZVOUS_URL)]
        room_url: String,
        #[arg(long, default_value = "browser-peer-downloads")]
        output_dir: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum RuntimeCommand {
    #[command(about = "Print runtime paths and disposable-launcher state")]
    Inspect {
        #[arg(long, default_value = ".altair-runtime-state")]
        state_name: String,
    },
}

#[derive(Debug, Subcommand)]
enum SyncCommand {
    #[command(hide = true)]
    #[command(about = "Print a content-addressed snapshot of a folder")]
    Snapshot { root: PathBuf },
    #[command(hide = true)]
    #[command(about = "Apply a local three-way sync merge")]
    MergeApply {
        base: PathBuf,
        local: PathBuf,
        remote: PathBuf,
    },
    #[command(hide = true)]
    #[command(about = "Watch a folder and print manifest changes")]
    Watch {
        root: PathBuf,
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
    },
    #[command(hide = true)]
    #[command(about = "Export a folder into an iroh-docs sync document")]
    DocsExport {
        root: PathBuf,
        #[arg(long)]
        state_dir: Option<PathBuf>,
    },
    #[command(hide = true)]
    #[command(about = "Export and continuously publish a folder")]
    DocsServe {
        root: PathBuf,
        #[arg(long)]
        code: Option<String>,
        #[arg(long, default_value = DEFAULT_RENDEZVOUS_URL)]
        room_url: String,
        #[arg(long, value_enum, default_value_t = PairMode::Persistent)]
        pair_mode: PairMode,
        #[arg(long)]
        naked: bool,
        #[arg(long)]
        state_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
    },
    #[command(hide = true)]
    #[command(about = "Import and print a docs sync manifest")]
    DocsImport {
        ticket: String,
        #[arg(long)]
        state_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 1500)]
        wait_ms: u64,
    },
    #[command(hide = true)]
    #[command(about = "Fetch one path from a docs sync ticket")]
    DocsFetch {
        ticket: String,
        path: String,
        #[arg(long)]
        state_dir: Option<PathBuf>,
        #[arg(long, default_value = "sync-fetch-output")]
        output_dir: PathBuf,
        #[arg(long, default_value_t = 1500)]
        wait_ms: u64,
    },
    #[command(hide = true)]
    #[command(about = "Apply a docs sync ticket into a local folder")]
    DocsApply {
        ticket: String,
        base: PathBuf,
        local: PathBuf,
        #[arg(long)]
        state_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 1500)]
        wait_ms: u64,
    },
    #[command(hide = true)]
    #[command(about = "Continuously follow a docs sync ticket")]
    DocsFollow {
        ticket: Option<String>,
        local: PathBuf,
        #[arg(long, default_value = DEFAULT_RENDEZVOUS_URL)]
        room_url: String,
        #[arg(long, value_enum, default_value_t = PairMode::Persistent)]
        pair_mode: PairMode,
        #[arg(long)]
        naked: bool,
        #[arg(long)]
        state_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 1500)]
        wait_ms: u64,
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
    },
    #[command(hide = true)]
    #[command(about = "Experimentally join a docs sync ticket bidirectionally")]
    DocsJoin {
        ticket: Option<String>,
        local: PathBuf,
        #[arg(long, default_value = DEFAULT_RENDEZVOUS_URL)]
        room_url: String,
        #[arg(long, value_enum, default_value_t = PairMode::Persistent)]
        pair_mode: PairMode,
        #[arg(long)]
        naked: bool,
        #[arg(long)]
        state_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 1500)]
        wait_ms: u64,
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
    },
}

#[derive(Debug)]
enum SyncRoleCommand {
    Host {
        folder: PathBuf,
        room_url: String,
        pair_mode: PairMode,
        naked: bool,
        qr: bool,
        state_dir: Option<PathBuf>,
        interval_ms: u64,
    },
    Follow {
        folder: PathBuf,
        code: Option<String>,
        room_url: String,
        pair_mode: PairMode,
        naked: bool,
        state_dir: Option<PathBuf>,
        wait_ms: u64,
        interval_ms: u64,
    },
    Join {
        folder: PathBuf,
        code: Option<String>,
        room_url: String,
        pair_mode: PairMode,
        naked: bool,
        state_dir: Option<PathBuf>,
        wait_ms: u64,
        interval_ms: u64,
    },
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum RoomServerEvent {
    Snapshot {
        peers: Vec<RoomPeerInfo>,
    },
    PeerJoined {
        #[serde(rename = "endpointId")]
        endpoint_id: String,
    },
    PeerLeft {
        #[serde(rename = "endpointId")]
        endpoint_id: String,
    },
    Relay {
        #[serde(rename = "fromEndpointId")]
        from_endpoint_id: String,
        payload: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoomPeerInfo {
    endpoint_id: String,
}

#[derive(Debug, Serialize)]
struct RoomRelayMessage<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(rename = "toEndpointId")]
    to_endpoint_id: &'a str,
    payload: SyncTicketPayload<'a>,
}

#[derive(Debug, Serialize)]
struct SyncTicketPayload<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    ticket: &'a str,
}

#[derive(Debug, Deserialize)]
struct ReceivedSyncTicketPayload {
    #[serde(rename = "type")]
    kind: String,
    ticket: String,
}

#[derive(Debug, Serialize)]
struct NativeRoomRelayMessage<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(rename = "toEndpointId")]
    to_endpoint_id: &'a str,
    payload: NativePairingPayload<'a>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum NativePairingPayload<'a> {
    Pake { payload: &'a [u8] },
    Bootstrap { envelope: PairingIntroEnvelope },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum ReceivedNativePairingPayload {
    Pake { payload: Vec<u8> },
    Bootstrap { envelope: PairingIntroEnvelope },
}

#[derive(Debug, Serialize, Deserialize)]
struct LocalPairingInit {
    pake: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LocalPairingReply {
    pake: Vec<u8>,
    bootstrap: PairingIntroEnvelope,
}

#[derive(Debug, Serialize, Deserialize)]
struct LocalPairingFinish {
    bootstrap: PairingIntroEnvelope,
}

struct NativeControlPeer {
    endpoint: Endpoint,
    pairing: EstablishedPairing,
    local_bundle: IrohBootstrapBundle,
    remote_bundle: IrohBootstrapBundle,
}

struct NativeBlobProvider {
    router: Router,
    ticket: BlobTicket,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct PairState {
    short_code: Option<String>,
    room_url: Option<String>,
    endpoint_ticket: Option<String>,
    blob_ticket: Option<String>,
    file_ticket: Option<String>,
    docs_ticket: Option<String>,
    mode: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct NakedFileTicket {
    blob_ticket: String,
    file_name: String,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PairMode {
    OneOff,
    Persistent,
}

enum SyncInputSource {
    Explicit,
    Saved,
}

struct SyncInput {
    value: String,
    source: SyncInputSource,
}

impl std::fmt::Display for PairMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairMode::OneOff => formatter.write_str("one-off"),
            PairMode::Persistent => formatter.write_str("persistent"),
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PeerKindArg {
    Cli,
    Web,
}

impl From<PeerKindArg> for MessagingPeerKind {
    fn from(value: PeerKindArg) -> Self {
        match value {
            PeerKindArg::Cli => MessagingPeerKind::Cli,
            PeerKindArg::Web => MessagingPeerKind::Web,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Send { command } => match command {
            SendCommand::Text {
                message,
                code,
                room_url,
                pair_mode,
                naked,
                qr,
            } => {
                if naked {
                    let ticket = resolve_endpoint_ticket(code)?;
                    run_naked_send_text(&ticket, message).await?;
                    return Ok(());
                }
                let code = resolve_short_code_or_generate_for_mode(code, pair_mode)?;
                save_short_pair_state(&code, &room_url, pair_mode)?;
                print_share_value("code", &code.to_string(), qr)?;
                println!("pair mode: {pair_mode}");
                run_native_send_text(code, room_url, message).await?;
            }
            SendCommand::File {
                path,
                code,
                room_url,
                pair_mode,
                naked,
                qr,
                state_dir,
            } => {
                if naked {
                    run_naked_send_file(path, state_dir, qr).await?;
                    return Ok(());
                }
                let code = resolve_short_code_or_generate_for_mode(code, pair_mode)?;
                save_short_pair_state(&code, &room_url, pair_mode)?;
                print_share_value("code", &code.to_string(), qr)?;
                println!("pair mode: {pair_mode}");
                run_native_send_file(code, room_url, path, state_dir).await?;
            }
        },
        Command::Receive { command } => match command {
            ReceiveCommand::Text {
                code,
                room_url,
                pair_mode,
                naked,
                qr,
            } => {
                if naked {
                    ensure!(
                        code.is_none(),
                        "receive text --naked does not accept a code"
                    );
                    run_naked_receive_text(qr).await?;
                    return Ok(());
                }
                let code = resolve_required_short_code(code)?;
                save_short_pair_state(&code, &room_url, pair_mode)?;
                println!("pair mode: {pair_mode}");
                run_native_receive_text(code, room_url).await?;
            }
            ReceiveCommand::File {
                code,
                output_dir,
                room_url,
                pair_mode,
                naked,
                state_dir,
            } => {
                if naked {
                    let file_ticket = resolve_file_ticket(code)?;
                    run_naked_receive_file(
                        &encode_naked_file_ticket(
                            &file_ticket.blob_ticket,
                            &file_ticket.file_name,
                        )?,
                        output_dir,
                        state_dir,
                    )
                    .await?;
                    return Ok(());
                }
                let code = resolve_required_short_code(code)?;
                save_short_pair_state(&code, &room_url, pair_mode)?;
                println!("pair mode: {pair_mode}");
                run_native_receive_file(code, room_url, output_dir, state_dir).await?;
            }
        },
        Command::Pair { args } => run_pair_command(args).await?,
        Command::Serve {
            command:
                ServeCommand::BrowserPeer {
                    code,
                    room_url,
                    output_dir,
                },
        }
        | Command::BrowserPeer {
            command:
                BrowserPeerCommand::Serve {
                    code,
                    room_url,
                    output_dir,
                },
        }
        | Command::Dev {
            command:
                DevCommand::BrowserPeer {
                    command:
                        BrowserPeerCommand::Serve {
                            code,
                            room_url,
                            output_dir,
                        },
                },
        } => {
            let code = ShortCode::from_str(&code).context("parse short code")?;
            browser_peer::run_browser_peer(code.normalized(), room_url, output_dir).await?;
        }
        Command::Code { command }
        | Command::Dev {
            command: DevCommand::Code { command },
        } => match command {
            CodeCommand::Generate => {
                let code = ShortCode::generate();
                println!("{code}");
            }
            CodeCommand::Inspect { code } => {
                let code = ShortCode::from_str(&code).context("parse short code")?;
                let [first, second, third] = code.words();
                println!("normalized: {}", code.normalized());
                println!("slot: {}", code.slot());
                println!("words: {first}, {second}, {third}");
                println!("pairing identity: {}", code.pairing_identity());
            }
        },
        Command::Pairing { command }
        | Command::Dev {
            command: DevCommand::Pairing { command },
        } => match command {
            PairingCommand::Demo { code } => {
                let code = match code {
                    Some(code) => ShortCode::from_str(&code).context("parse short code")?,
                    None => ShortCode::generate(),
                };

                println!("using code: {code}");
                let outcome = run_local_pairing_probe(code.clone()).await?;
                println!("pairing bootstrap succeeded");
                println!("left ticket: {}", outcome.left_ticket);
                println!("right ticket: {}", outcome.right_ticket);
            }
        },
        Command::Message { command }
        | Command::Dev {
            command: DevCommand::Message { command },
        } => match command {
            MessageCommand::Demo {
                code,
                left,
                right,
                left_text,
                right_text,
            } => {
                let code = match code {
                    Some(code) => ShortCode::from_str(&code).context("parse short code")?,
                    None => ShortCode::generate(),
                };
                let outcome = run_local_message_probe(
                    code.clone(),
                    left.into(),
                    right.into(),
                    left_text,
                    right_text,
                )
                .await?;

                println!("using code: {}", outcome.code);
                println!("left peer kind: {:?}", outcome.left_kind);
                println!("right peer kind: {:?}", outcome.right_kind);
                println!("left sent: {}", outcome.left_sent);
                println!("right received: {}", outcome.right_received);
                println!("right sent: {}", outcome.right_sent);
                println!("left received: {}", outcome.left_received);
            }
        },
        Command::File { command }
        | Command::Dev {
            command: DevCommand::File { command },
        } => match command {
            FileCommand::Demo {
                code,
                left,
                right,
                name,
                path,
                text,
                receiver_state_root,
                interrupt_after_chunks,
            } => {
                let code = match code {
                    Some(code) => ShortCode::from_str(&code).context("parse short code")?,
                    None => ShortCode::generate(),
                };
                let payload = match path {
                    Some(path) => std::fs::read(&path)
                        .with_context(|| format!("read demo file at {}", path.display()))?,
                    None => text.into_bytes(),
                };
                let outcome = if receiver_state_root.is_some() || interrupt_after_chunks.is_some() {
                    run_local_file_probe_with_config(
                        code.clone(),
                        left.into(),
                        right.into(),
                        name,
                        &payload,
                        FileProbeMode::Accept,
                        FileProbeConfig {
                            receiver_state_root,
                            interrupt_after_chunks,
                        },
                    )
                    .await?
                } else {
                    run_local_file_probe(
                        code.clone(),
                        left.into(),
                        right.into(),
                        name,
                        &payload,
                        FileProbeMode::Accept,
                    )
                    .await?
                };

                println!("using code: {}", outcome.code);
                println!("left peer kind: {:?}", outcome.left_kind);
                println!("right peer kind: {:?}", outcome.right_kind);
                println!("file: {}", outcome.file_name);
                println!("transport: {:?}", outcome.transport);
                println!("resumed local bytes: {}", outcome.resumed_local_bytes);
                println!("bytes sent: {}", outcome.bytes_sent);
                println!("bytes received: {}", outcome.bytes_received);
                println!("accepted: {}", outcome.accepted);
                println!("cancelled: {}", outcome.cancelled);
            }
            FileCommand::NativeResumeDemo {
                code,
                name,
                path,
                text,
                seeded_chunks,
                receiver_state_root,
            } => {
                let code = match code {
                    Some(code) => ShortCode::from_str(&code).context("parse short code")?,
                    None => ShortCode::generate(),
                };
                let payload = match path {
                    Some(path) => std::fs::read(&path).with_context(|| {
                        format!("read native resume demo file at {}", path.display())
                    })?,
                    None => text.into_bytes(),
                };
                let outcome = run_local_native_resume_probe(
                    code.clone(),
                    name,
                    &payload,
                    seeded_chunks,
                    receiver_state_root,
                )
                .await?;

                println!("using code: {}", outcome.code);
                println!("file: {}", outcome.file_name);
                println!("seeded chunks: {}", outcome.seeded_chunks);
                println!("initial local bytes: {}", outcome.initial_local_bytes);
                println!("final bytes: {}", outcome.final_bytes);
                println!("expected hash: {:02x?}", outcome.expected_hash);
                println!("received hash: {:02x?}", outcome.received_hash);
            }
        },
        Command::Runtime { command } => match command {
            RuntimeCommand::Inspect { state_name } => {
                let current_exe = std::env::current_exe().context("resolve current executable")?;
                println!("current exe: {}", current_exe.display());
                println!("temp dir: {}", std::env::temp_dir().display());
                println!(
                    "preferred runtime parent: {}",
                    preferred_runtime_parent().display()
                );
                match runtime_root_from_env() {
                    Some(root) => println!("runtime root: {}", root.display()),
                    None => println!("runtime root: <none>"),
                }
                println!(
                    "resolved state dir: {}",
                    resolve_runtime_state_dir(None, &state_name).display()
                );
                println!("keep runtime requested: {}", keep_runtime_requested());
            }
        },
        Command::Help { no_pager, topic } => {
            let mut no_pager = no_pager;
            let topic = topic
                .into_iter()
                .filter(|item| {
                    if item == "--no-pager" {
                        no_pager = true;
                        false
                    } else {
                        true
                    }
                })
                .collect::<Vec<_>>();
            print_manual(&topic, no_pager)?;
        }
        Command::Sync { command, args } => {
            let command = match command {
                Some(command) => command,
                None => match infer_sync_command(args)? {
                    SyncRoleCommand::Host {
                        folder: root,
                        room_url,
                        pair_mode,
                        naked,
                        qr,
                        state_dir,
                        interval_ms,
                    } => {
                        let code = resolve_short_code_or_generate(None)?;
                        ensure!(
                            matches!(pair_mode, PairMode::Persistent),
                            "sync requires --pair-mode persistent when hosting"
                        );
                        run_sync_host(root, code, room_url, state_dir, interval_ms, naked, qr)
                            .await?;
                        return Ok(());
                    }
                    SyncRoleCommand::Follow {
                        folder: local,
                        code: ticket,
                        room_url,
                        pair_mode,
                        naked,
                        state_dir,
                        wait_ms,
                        interval_ms,
                    } => SyncCommand::DocsFollow {
                        ticket,
                        local,
                        room_url,
                        pair_mode,
                        naked,
                        state_dir,
                        wait_ms,
                        interval_ms,
                    },
                    SyncRoleCommand::Join {
                        folder: local,
                        code: ticket,
                        room_url,
                        pair_mode,
                        naked,
                        state_dir,
                        wait_ms,
                        interval_ms,
                    } => SyncCommand::DocsJoin {
                        ticket,
                        local,
                        room_url,
                        pair_mode,
                        naked,
                        state_dir,
                        wait_ms,
                        interval_ms,
                    },
                },
            };
            match command {
                SyncCommand::Snapshot { root } => {
                    let manifest =
                        scan_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                            .with_context(|| format!("scan sync root {}", root.display()))?;
                    println!("root: {}", root.display());
                    println!("entries: {}", manifest.len());
                    for entry in manifest.entries.values() {
                        match &entry.state {
                            altair_vega::SyncEntryState::File(descriptor) => {
                                println!(
                                    "file {} {} {:02x?}",
                                    entry.path,
                                    descriptor.size_bytes,
                                    &descriptor.hash[..4]
                                );
                            }
                            altair_vega::SyncEntryState::Tombstone => {
                                println!("tombstone {}", entry.path);
                            }
                        }
                    }
                }
                SyncCommand::MergeApply {
                    base,
                    local,
                    remote,
                } => {
                    let base_manifest =
                        scan_directory(&base, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                            .with_context(|| format!("scan base sync root {}", base.display()))?;
                    let local_manifest =
                        scan_directory(&local, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                            .with_context(|| format!("scan local sync root {}", local.display()))?;
                    let remote_manifest =
                        scan_directory(&remote, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                            .with_context(|| {
                                format!("scan remote sync root {}", remote.display())
                            })?;
                    let plan = merge_manifests(&base_manifest, &local_manifest, &remote_manifest);
                    println!("base: {}", base.display());
                    println!("local: {}", local.display());
                    println!("remote: {}", remote.display());
                    println!("actions: {}", plan.actions.len());
                    println!("conflicts: {}", plan.conflicts.len());
                    apply_merge_plan(&local, &remote, &plan)
                        .with_context(|| format!("apply merge plan into {}", local.display()))?;
                }
                SyncCommand::Watch { root, interval_ms } => {
                    let mut previous =
                        scan_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                            .with_context(|| format!("scan sync watch root {}", root.display()))?;
                    println!("watching: {}", root.display());
                    println!("interval ms: {interval_ms}");
                    println!("press Ctrl+C to stop");
                    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
                    let mut watcher = notify::recommended_watcher(move |event| {
                        let _ = event_tx.send(event);
                    })
                    .context("create filesystem watcher")?;
                    watcher
                        .watch(&root, RecursiveMode::Recursive)
                        .with_context(|| format!("watch sync root {}", root.display()))?;
                    let mut interval =
                        tokio::time::interval(std::time::Duration::from_millis(interval_ms));
                    loop {
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => break,
                            maybe_event = event_rx.recv() => {
                                if let Some(event) = maybe_event {
                                    match event {
                                        Ok(event) => {
                                            if matches!(event.kind, EventKind::Access(_)) {
                                                continue;
                                            }
                                            println!("watch event: {:?}", event.kind);
                                        }
                                        Err(error) => {
                                            println!("watch error: {error}");
                                        }
                                    }
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                                let current = scan_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                                    .with_context(|| format!("rescan sync watch root {}", root.display()))?;
                                for change in diff_manifests(&previous, &current) {
                                    println!("{:?} {}", change.kind, change.path);
                                }
                                previous = current;
                            }
                            _ = interval.tick() => {
                                let current = scan_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                                    .with_context(|| format!("rescan sync watch root {}", root.display()))?;
                                for change in diff_manifests(&previous, &current) {
                                    println!("{:?} {}", change.kind, change.path);
                                }
                                previous = current;
                            }
                        }
                    }
                }
                SyncCommand::DocsExport { root, state_dir } => {
                    let state_dir = resolve_runtime_state_dir(state_dir, ".altair-sync-docs");
                    let node = sync_docs::DocsSyncNode::spawn_persistent(&state_dir).await?;
                    let result = node
                        .export_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                        .await?;
                    println!("root: {}", root.display());
                    println!("state dir: {}", state_dir.display());
                    println!("doc id: {}", result.doc_id);
                    println!("ticket: {}", result.ticket);
                    println!("entries: {}", result.manifest.len());
                    println!("content blobs: {}", result.content_blobs);
                    for line in sync_docs::summarize_manifest(&result.manifest) {
                        println!("{line}");
                    }
                    println!("press Ctrl+C to stop serving this doc");
                    tokio::signal::ctrl_c().await?;
                    node.shutdown().await?;
                }
                SyncCommand::DocsServe {
                    root,
                    code,
                    room_url,
                    pair_mode: _,
                    naked: _,
                    state_dir,
                    interval_ms,
                } => {
                    let sync_code = match code {
                        Some(code) => {
                            Some(ShortCode::from_str(&code).context("parse sync short code")?)
                        }
                        None => None,
                    };
                    let state_dir = resolve_runtime_state_dir(state_dir, ".altair-sync-docs-serve");
                    let node = sync_docs::DocsSyncNode::spawn_persistent(&state_dir).await?;
                    let current_manifest =
                        scan_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                            .with_context(|| format!("scan sync serve root {}", root.display()))?;
                    let manifest_state_path = state_dir.join(format!(
                        "host-{}-last-published-manifest.json",
                        state_key(&root.display().to_string())
                    ));
                    let doc_state_path = host_doc_state_path(&state_dir, &root);
                    let previous_manifest = if manifest_state_path.exists() {
                        load_manifest_state(&manifest_state_path, "host publish")?
                    } else {
                        current_manifest.clone()
                    };
                    let (doc, result) = export_host_manifest(
                        &node,
                        &doc_state_path,
                        &root,
                        &previous_manifest,
                        current_manifest.clone(),
                    )
                    .await?;
                    println!("root: {}", root.display());
                    println!("state dir: {}", state_dir.display());
                    println!("doc id: {}", result.doc_id);
                    println!("ticket: {}", result.ticket);
                    if let Some(code) = &sync_code {
                        println!("code: {code}");
                        println!("rendezvous: {room_url}");
                    }
                    println!("entries: {}", result.manifest.len());
                    println!("content blobs: {}", result.content_blobs);
                    println!("watch interval ms: {interval_ms}");
                    println!("press Ctrl+C to stop");

                    persist_manifest_state(&manifest_state_path, &current_manifest)?;
                    let mut published_manifest = current_manifest;
                    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
                    let mut watcher = notify::recommended_watcher(move |event| {
                        let _ = event_tx.send(event);
                    })
                    .context("create docs serve watcher")?;
                    watcher
                        .watch(&root, RecursiveMode::Recursive)
                        .with_context(|| format!("watch docs serve root {}", root.display()))?;
                    let mut interval =
                        tokio::time::interval(std::time::Duration::from_millis(interval_ms));
                    loop {
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => break,
                            maybe_event = event_rx.recv() => {
                                if let Some(Ok(event)) = maybe_event
                                    && matches!(event.kind, EventKind::Access(_)) {
                                    continue;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                                let current = scan_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                                    .with_context(|| format!("rescan docs serve root {}", root.display()))?;
                                let changes = diff_manifests(&published_manifest, &current);
                                if changes.is_empty() {
                                    continue;
                                }
                                match node.publish_manifest(&doc, &root, &published_manifest, &current).await {
                                    Ok((content_blobs, next_manifest)) => {
                                        println!("published changes: {} content blobs: {}", changes.len(), content_blobs);
                                        print_sync_changes(&changes);
                                        persist_manifest_state(&manifest_state_path, &next_manifest)?;
                                        published_manifest = next_manifest;
                                    }
                                    Err(error) => {
                                        println!("publish error: {error}");
                                    }
                                }
                            }
                            _ = interval.tick() => {
                                let current = scan_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                                    .with_context(|| format!("rescan docs serve root {}", root.display()))?;
                                let changes = diff_manifests(&published_manifest, &current);
                                if changes.is_empty() {
                                    continue;
                                }
                                match node.publish_manifest(&doc, &root, &published_manifest, &current).await {
                                    Ok((content_blobs, next_manifest)) => {
                                        println!("published changes: {} content blobs: {}", changes.len(), content_blobs);
                                        print_sync_changes(&changes);
                                        persist_manifest_state(&manifest_state_path, &next_manifest)?;
                                        published_manifest = next_manifest;
                                    }
                                    Err(error) => {
                                        println!("publish error: {error}");
                                    }
                                }
                            }
                        }
                    }
                    node.shutdown().await?;
                }
                SyncCommand::DocsImport {
                    ticket,
                    state_dir,
                    wait_ms,
                } => {
                    let state_dir =
                        resolve_runtime_state_dir(state_dir, ".altair-sync-docs-import");
                    let node = sync_docs::DocsSyncNode::spawn_persistent(&state_dir).await?;
                    let manifest = node.import_manifest(&ticket, wait_ms).await?;
                    println!("state dir: {}", state_dir.display());
                    println!("entries: {}", manifest.len());
                    for line in sync_docs::summarize_manifest(&manifest) {
                        println!("{line}");
                    }
                    node.shutdown().await?;
                }
                SyncCommand::DocsFetch {
                    ticket,
                    path,
                    state_dir,
                    output_dir,
                    wait_ms,
                } => {
                    let state_dir = resolve_runtime_state_dir(state_dir, ".altair-sync-docs-fetch");
                    let node = sync_docs::DocsSyncNode::spawn_persistent(&state_dir).await?;
                    let manifest = node
                        .fetch_path_from_ticket(&ticket, &path, &output_dir, wait_ms)
                        .await?;
                    println!("state dir: {}", state_dir.display());
                    println!("output dir: {}", output_dir.display());
                    println!("fetched path: {path}");
                    println!("entries: {}", manifest.len());
                    node.shutdown().await?;
                }
                SyncCommand::DocsApply {
                    ticket,
                    base,
                    local,
                    state_dir,
                    wait_ms,
                } => {
                    let state_dir = resolve_runtime_state_dir(state_dir, ".altair-sync-docs-apply");
                    let node = sync_docs::DocsSyncNode::spawn_persistent(&state_dir).await?;
                    let plan = node
                        .apply_ticket_merge(&ticket, &base, &local, wait_ms)
                        .await?;
                    println!("base: {}", base.display());
                    println!("local: {}", local.display());
                    println!("state dir: {}", state_dir.display());
                    println!("actions: {}", plan.actions.len());
                    println!("conflicts: {}", plan.conflicts.len());
                    node.shutdown().await?;
                }
                SyncCommand::DocsFollow {
                    ticket,
                    local,
                    room_url,
                    pair_mode,
                    naked,
                    state_dir,
                    wait_ms,
                    interval_ms,
                } => {
                    let sync_input = resolve_sync_input(ticket, naked)?;
                    let ticket = sync_input.value;
                    if matches!(sync_input.source, SyncInputSource::Saved) {
                        println!(
                            "using saved {} from pair state",
                            if naked { "docs ticket" } else { "short code" }
                        );
                    }
                    if naked {
                        save_docs_pair_state(&ticket, pair_mode)?;
                    } else if let Ok(code) = ShortCode::from_str(&ticket) {
                        save_short_pair_state(&code, &room_url, pair_mode)?;
                    }
                    let ticket = resolve_sync_ticket_or_code(&ticket, &room_url, naked).await?;
                    let state_root =
                        resolve_runtime_state_dir(state_dir, ".altair-sync-docs-follow");
                    let state_dir = state_root.join(format!(
                        "follow-{}-{}",
                        state_key(&ticket),
                        state_key(&local.display().to_string())
                    ));
                    std::fs::create_dir_all(&local)
                        .with_context(|| format!("create follow local root {}", local.display()))?;
                    let node = sync_docs::DocsSyncNode::spawn_persistent(&state_dir).await?;
                    let sync_state_path = state_dir.join("base-manifest.json");
                    let had_sync_state = sync_state_path.exists();
                    let mut base_manifest = if had_sync_state {
                        load_manifest_state(&sync_state_path, "follow base")?
                    } else {
                        scan_directory(&local, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                            .with_context(|| {
                                format!("scan follow local root {}", local.display())
                            })?
                    };
                    let imported = node.import_doc(&ticket).await?;
                    tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                    let initial_local =
                        scan_directory(&local, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                            .with_context(|| {
                                format!("scan initial follow local root {}", local.display())
                            })?;
                    let remote_manifest = wait_for_remote_manifest(
                        &node,
                        &imported.doc,
                        wait_ms,
                        initial_local.is_empty() && !had_sync_state,
                    )
                    .await?;
                    if !had_sync_state && initial_local.is_empty() {
                        let applied = node
                            .seed_local_from_manifest(
                                imported.peer.clone(),
                                &local,
                                &remote_manifest,
                            )
                            .await?;
                        println!("seeded files: {}", applied);
                        persist_manifest_state(&sync_state_path, &remote_manifest)?;
                        base_manifest = remote_manifest;
                    }
                    println!("following docs ticket into {}", local.display());
                    println!("state dir: {}", state_dir.display());
                    println!("interval ms: {interval_ms}");
                    println!("press Ctrl+C to stop");
                    let mut interval =
                        tokio::time::interval(std::time::Duration::from_millis(interval_ms));
                    let mut remote_events = Box::pin(imported.doc.subscribe().await?);
                    loop {
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => break,
                            maybe_remote = remote_events.next() => {
                                if maybe_remote.is_none() {
                                    continue;
                                }
                                let remote_manifest = node.read_doc_manifest(&imported.doc).await?;
                                let plan = node
                                    .apply_remote_manifest(imported.peer.clone(), &base_manifest, &local, &remote_manifest)
                                    .await?;
                                if plan.actions.is_empty() && plan.conflicts.is_empty() {
                                    if !manifests_state_eq(&base_manifest, &remote_manifest) {
                                        persist_manifest_state(&sync_state_path, &remote_manifest)?;
                                        base_manifest = remote_manifest;
                                    }
                                    continue;
                                }
                                print_sync_plan("applied", &plan);
                                persist_manifest_state(&sync_state_path, &remote_manifest)?;
                                base_manifest = remote_manifest;
                            }
                            _ = interval.tick() => {
                                let remote_manifest = node.read_doc_manifest(&imported.doc).await?;
                                let plan = node
                                    .apply_remote_manifest(imported.peer.clone(), &base_manifest, &local, &remote_manifest)
                                    .await?;
                                if plan.actions.is_empty() && plan.conflicts.is_empty() {
                                    continue;
                                }
                                print_sync_plan("applied", &plan);
                                persist_manifest_state(&sync_state_path, &remote_manifest)?;
                                base_manifest = remote_manifest;
                            }
                        }
                    }
                    node.shutdown().await?;
                }
                SyncCommand::DocsJoin {
                    ticket,
                    local,
                    room_url,
                    pair_mode,
                    naked,
                    state_dir,
                    wait_ms,
                    interval_ms,
                } => {
                    let sync_input = resolve_sync_input(ticket, naked)?;
                    let ticket = sync_input.value;
                    if matches!(sync_input.source, SyncInputSource::Saved) {
                        println!(
                            "using saved {} from pair state",
                            if naked { "docs ticket" } else { "short code" }
                        );
                    }
                    if naked {
                        save_docs_pair_state(&ticket, pair_mode)?;
                    } else if let Ok(code) = ShortCode::from_str(&ticket) {
                        save_short_pair_state(&code, &room_url, pair_mode)?;
                    }
                    let ticket = resolve_sync_ticket_or_code(&ticket, &room_url, naked).await?;
                    let state_root = resolve_runtime_state_dir(state_dir, ".altair-sync-docs-join");
                    let state_dir = state_root.join(format!(
                        "join-{}-{}",
                        state_key(&ticket),
                        state_key(&local.display().to_string())
                    ));
                    std::fs::create_dir_all(&local)
                        .with_context(|| format!("create join local root {}", local.display()))?;
                    let node = sync_docs::DocsSyncNode::spawn_persistent(&state_dir).await?;
                    let sync_state_path = state_dir.join("base-manifest.json");
                    let had_sync_state = sync_state_path.exists();
                    let mut base_manifest = if had_sync_state {
                        load_manifest_state(&sync_state_path, "join base")?
                    } else {
                        altair_vega::SyncManifest::default()
                    };
                    let imported = node.import_doc(&ticket).await?;
                    tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                    let initial_local =
                        scan_directory(&local, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                            .with_context(|| {
                                format!("scan initial join local root {}", local.display())
                            })?;
                    let initial_remote = wait_for_remote_manifest(
                        &node,
                        &imported.doc,
                        wait_ms,
                        initial_local.is_empty() && !had_sync_state,
                    )
                    .await?;
                    let initial_plan = if !had_sync_state && initial_local.is_empty() {
                        let applied = node
                            .seed_local_from_manifest(
                                imported.peer.clone(),
                                &local,
                                &initial_remote,
                            )
                            .await?;
                        println!("initial seeded files: {}", applied);
                        altair_vega::SyncMergePlan::default()
                    } else {
                        node.apply_remote_manifest(
                            imported.peer.clone(),
                            &base_manifest,
                            &local,
                            &initial_remote,
                        )
                        .await?
                    };
                    println!("joined docs ticket into {}", local.display());
                    println!("state dir: {}", state_dir.display());
                    println!("interval ms: {interval_ms}");
                    println!(
                        "initial actions: {} conflicts: {}",
                        initial_plan.actions.len(),
                        initial_plan.conflicts.len()
                    );
                    persist_manifest_state(&sync_state_path, &initial_remote)?;
                    base_manifest = initial_remote;
                    let mut last_published_manifest: Option<altair_vega::SyncManifest> = None;
                    println!("press Ctrl+C to stop");

                    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
                    let mut watcher = notify::recommended_watcher(move |event| {
                        let _ = event_tx.send(event);
                    })
                    .context("create docs join watcher")?;
                    watcher
                        .watch(&local, RecursiveMode::Recursive)
                        .with_context(|| {
                            format!("watch docs join local root {}", local.display())
                        })?;
                    let mut interval =
                        tokio::time::interval(std::time::Duration::from_millis(interval_ms));
                    let mut remote_events = Box::pin(imported.doc.subscribe().await?);

                    loop {
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => break,
                            maybe_remote = remote_events.next() => {
                                if maybe_remote.is_none() {
                                    println!("remote sync subscription ended; reconnecting");
                                    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                                    remote_events = Box::pin(imported.doc.subscribe().await?);
                                    continue;
                                }
                                if let Err(error) = reconcile_join_remote(
                                    &node,
                                    &imported.doc,
                                    &imported.peer,
                                    &local,
                                    &sync_state_path,
                                    &mut base_manifest,
                                    &mut last_published_manifest,
                                )
                                .await {
                                    println!("remote sync error: {error}");
                                }
                            }
                            maybe_event = event_rx.recv() => {
                                if let Some(Ok(event)) = maybe_event && matches!(event.kind, EventKind::Access(_)) {
                                    continue;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                                if let Err(error) = reconcile_join_remote(
                                    &node,
                                    &imported.doc,
                                    &imported.peer,
                                    &local,
                                    &sync_state_path,
                                    &mut base_manifest,
                                    &mut last_published_manifest,
                                )
                                .await {
                                    println!("remote sync error: {error}");
                                    continue;
                                }
                                publish_join_local_changes(
                                    &node,
                                    &imported.doc,
                                    &local,
                                    &base_manifest,
                                    &mut last_published_manifest,
                                )
                                .await?;
                            }
                            _ = interval.tick() => {
                                if let Err(error) = reconcile_join_remote(
                                    &node,
                                    &imported.doc,
                                    &imported.peer,
                                    &local,
                                    &sync_state_path,
                                    &mut base_manifest,
                                    &mut last_published_manifest,
                                )
                                .await {
                                    println!("remote sync error: {error}");
                                    continue;
                                }
                                publish_join_local_changes(
                                    &node,
                                    &imported.doc,
                                    &local,
                                    &base_manifest,
                                    &mut last_published_manifest,
                                )
                                .await?;
                            }
                        }
                    }
                    node.shutdown().await?;
                }
            }
        }
    }

    Ok(())
}

fn pair_state_path() -> PathBuf {
    resolve_runtime_state_dir(None, ".altair-pair").join("pair-state.json")
}

fn load_pair_state() -> Result<Option<PairState>> {
    let path = pair_state_path();
    if !path.exists() {
        return Ok(None);
    }
    let state = serde_json::from_slice::<PairState>(
        &fs::read(&path).with_context(|| format!("read pair state {}", path.display()))?,
    )
    .with_context(|| format!("deserialize pair state {}", path.display()))?;
    Ok(Some(state))
}

fn persist_pair_state(state: &PairState) -> Result<()> {
    let path = pair_state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create pair state dir {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_vec_pretty(state)?)
        .with_context(|| format!("write pair state {}", path.display()))?;
    Ok(())
}

fn save_short_pair_state(code: &ShortCode, room_url: &str, mode: PairMode) -> Result<()> {
    let mut state = load_pair_state()?.unwrap_or_default();
    state.short_code = Some(code.to_string());
    state.room_url = Some(room_url.to_string());
    state.mode = Some(mode.to_string());
    persist_pair_state(&state)
}

fn save_endpoint_pair_state(ticket: &str, mode: PairMode) -> Result<()> {
    let mut state = load_pair_state()?.unwrap_or_default();
    state.endpoint_ticket = Some(ticket.to_string());
    state.mode = Some(mode.to_string());
    persist_pair_state(&state)
}

fn save_docs_pair_state(ticket: &str, mode: PairMode) -> Result<()> {
    let mut state = load_pair_state()?.unwrap_or_default();
    state.docs_ticket = Some(ticket.to_string());
    state.mode = Some(mode.to_string());
    persist_pair_state(&state)
}

fn save_blob_pair_state(blob_ticket: &str, file_ticket: &str, mode: PairMode) -> Result<()> {
    let mut state = load_pair_state()?.unwrap_or_default();
    state.blob_ticket = Some(blob_ticket.to_string());
    state.file_ticket = Some(file_ticket.to_string());
    state.mode = Some(mode.to_string());
    persist_pair_state(&state)
}

fn resolve_short_code_or_generate(code: Option<String>) -> Result<ShortCode> {
    if let Some(code) = code {
        return ShortCode::from_str(&code).context("parse short code");
    }
    if let Some(state) = load_pair_state()?
        && let Some(code) = state.short_code
    {
        return ShortCode::from_str(&code).context("parse pair state short code");
    }
    Ok(ShortCode::generate())
}

fn resolve_short_code_or_generate_for_mode(
    code: Option<String>,
    mode: PairMode,
) -> Result<ShortCode> {
    if let Some(code) = code {
        return ShortCode::from_str(&code).context("parse short code");
    }
    if matches!(mode, PairMode::Persistent) {
        return resolve_short_code_or_generate(None);
    }
    Ok(ShortCode::generate())
}

fn resolve_required_short_code(code: Option<String>) -> Result<ShortCode> {
    if let Some(code) = code {
        return ShortCode::from_str(&code).context("parse short code");
    }
    let state = load_pair_state()?.context("missing <CODE> and no saved pair state")?;
    let code = state.short_code.context(
        "saved pair state does not contain a short code; provide <CODE> or use the matching --naked command",
    )?;
    ShortCode::from_str(&code).context("parse pair state short code")
}

fn resolve_endpoint_ticket(ticket: Option<String>) -> Result<String> {
    if let Some(ticket) = ticket {
        return Ok(ticket);
    }
    let state = load_pair_state()?.context("missing <ENDPOINT_TICKET> and no saved pair state")?;
    state.endpoint_ticket.context(
        "saved pair state does not contain an endpoint ticket; provide <ENDPOINT_TICKET> or run a naked endpoint command first",
    )
}

fn resolve_file_ticket(ticket: Option<String>) -> Result<NakedFileTicket> {
    if let Some(ticket) = ticket {
        return parse_naked_file_ticket(&ticket);
    }
    let state = load_pair_state()?.context("missing <FILE_TICKET> and no saved pair state")?;
    if let Some(ticket) = state.file_ticket {
        return parse_naked_file_ticket(&ticket);
    }
    let blob_ticket = state.blob_ticket.context(
        "saved pair state does not contain a file/blob ticket; provide <FILE_TICKET> or run send file --naked first",
    )?;
    Ok(NakedFileTicket {
        blob_ticket,
        file_name: "received-blob.bin".to_string(),
    })
}

fn resolve_sync_input(input: Option<String>, naked: bool) -> Result<SyncInput> {
    if let Some(input) = input {
        return Ok(SyncInput {
            value: input,
            source: SyncInputSource::Explicit,
        });
    }
    let state = load_pair_state()?.context(if naked {
        "missing <DOCS_TICKET> and no saved pair state"
    } else {
        "missing <CODE> and no saved pair state"
    })?;
    let value = if naked {
        state.docs_ticket.context(
            "saved pair state does not contain a docs ticket; provide <DOCS_TICKET> or run sync --naked <FOLDER> first",
        )?
    } else {
        state.short_code.context(
            "saved pair state does not contain a short code; provide <CODE> or use --naked with a docs ticket",
        )?
    };
    Ok(SyncInput {
        value,
        source: SyncInputSource::Saved,
    })
}

fn infer_sync_command(args: SyncArgs) -> Result<SyncRoleCommand> {
    let folder = args
        .folder
        .context("sync requires <FOLDER>, or use an explicit sync subcommand")?;
    if args.join {
        return Ok(SyncRoleCommand::Join {
            folder,
            code: args.key,
            room_url: args.room_url,
            pair_mode: args.pair_mode,
            naked: args.naked,
            state_dir: args.state_dir,
            wait_ms: args.wait_ms,
            interval_ms: args.interval_ms,
        });
    }
    match args.key {
        Some(code) => Ok(SyncRoleCommand::Follow {
            folder,
            code: Some(code),
            room_url: args.room_url,
            pair_mode: args.pair_mode,
            naked: args.naked,
            state_dir: args.state_dir,
            wait_ms: args.wait_ms,
            interval_ms: args.interval_ms,
        }),
        None => Ok(SyncRoleCommand::Host {
            folder,
            room_url: args.room_url,
            pair_mode: args.pair_mode,
            naked: args.naked,
            qr: args.qr,
            state_dir: args.state_dir,
            interval_ms: args.interval_ms,
        }),
    }
}

fn encode_naked_file_ticket(blob_ticket: &str, file_name: &str) -> Result<String> {
    let payload = NakedFileTicket {
        blob_ticket: blob_ticket.to_string(),
        file_name: file_name.to_string(),
    };
    Ok(format!(
        "altair-vega-file-v1:{}",
        general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload)?)
    ))
}

fn parse_naked_file_ticket(value: &str) -> Result<NakedFileTicket> {
    let Some(encoded) = value.strip_prefix("altair-vega-file-v1:") else {
        BlobTicket::from_str(value).context("parse naked blob ticket")?;
        return Ok(NakedFileTicket {
            blob_ticket: value.to_string(),
            file_name: "received-blob.bin".to_string(),
        });
    };
    let bytes = general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .context("decode naked file ticket")?;
    let ticket =
        serde_json::from_slice::<NakedFileTicket>(&bytes).context("parse naked file ticket")?;
    BlobTicket::from_str(&ticket.blob_ticket).context("parse naked blob ticket")?;
    ensure!(
        !ticket.file_name.trim().is_empty(),
        "naked file ticket has empty file name"
    );
    Ok(ticket)
}

fn safe_ticket_file_name(file_name: &str) -> String {
    Path::new(file_name)
        .file_name()
        .and_then(|item| item.to_str())
        .filter(|item| !item.is_empty())
        .unwrap_or("received-blob.bin")
        .to_string()
}

fn normalize_optional_code_args(parts: Vec<String>) -> Result<Option<String>> {
    match parts.len() {
        0 => Ok(None),
        1 => Ok(parts.into_iter().next()),
        4 => Ok(Some(format!(
            "{}-{}-{}-{}",
            parts[0], parts[1], parts[2], parts[3]
        ))),
        _ => bail!("expected CODE as one dash-separated value or four space-separated parts"),
    }
}

fn print_manual(topic: &[String], no_pager: bool) -> Result<()> {
    let topic = topic.join(" ").to_ascii_lowercase();
    let text = match topic.as_str() {
        "" | "overview" => ALTAIR_VEGA_MANUAL,
        "pair" | "pairing" => ALTAIR_VEGA_PAIR_HELP,
        "send" | "receive" | "transfer" | "transfers" => ALTAIR_VEGA_TRANSFER_HELP,
        "sync" => ALTAIR_VEGA_SYNC_HELP,
        "serve" | "browser" | "browser-peer" => ALTAIR_VEGA_SERVE_HELP,
        "runtime" | "disposable" => ALTAIR_VEGA_RUNTIME_HELP,
        "examples" | "example" => ALTAIR_VEGA_EXAMPLES_HELP,
        _ => bail!(
            "unknown help topic '{topic}'. Try: overview, pair, transfer, sync, serve, runtime, examples"
        ),
    };
    print_help_text(text, no_pager)?;
    Ok(())
}

fn print_help_text(text: &str, no_pager: bool) -> Result<()> {
    if no_pager || !std::io::stdout().is_terminal() {
        print!("{text}");
        return Ok(());
    }

    let pager = std::env::var("PAGER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "less".to_string());
    let mut pager_parts = pager.split_whitespace();
    let Some(pager_program) = pager_parts.next() else {
        print!("{text}");
        return Ok(());
    };
    let mut child = match std::process::Command::new(pager_program)
        .args(pager_parts)
        .env("LESS", "FRX")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            print!("{text}");
            return Ok(());
        }
    };

    if let Some(mut stdin) = child.stdin.take()
        && stdin.write_all(text.as_bytes()).is_err()
    {
        print!("{text}");
        return Ok(());
    }

    if child.wait().is_err() {
        print!("{text}");
    }
    Ok(())
}

const ALTAIR_VEGA_MANUAL: &str = r#"ALTAIR-VEGA(1)

NAME
    altair-vega - peer-to-peer transfer and folder sync over short pairing codes

SYNOPSIS
    altair-vega send text [OPTIONS] <MESSAGE> [CODE]
    altair-vega send file [OPTIONS] <PATH> [CODE]
    altair-vega receive text [OPTIONS] [CODE]
    altair-vega receive file [OPTIONS] [CODE]
    altair-vega pair [OPTIONS] [CODE]
    altair-vega sync [OPTIONS] <FOLDER> [KEY]

DESCRIPTION
    Altair Vega uses a short human-typed code as the room key for rendezvous.
    Peers join the same room, exchange PAKE pairing material, then exchange
    encrypted iroh bootstrap details. Text, file transfer, and native folder
    sync are built on that shared room primitive.

    Commands reuse saved pair state when CODE or a naked ticket is omitted.
    The saved state is updated by pair and by transfer/sync commands that
    receive or create a code or naked ticket.

    Use -h or --help for terse argument reference. Use this help command for
    task-oriented guidance and examples.

TOPICS
    altair-vega help pair
    altair-vega help transfer
    altair-vega help sync
    altair-vega help serve
    altair-vega help runtime
    altair-vega help examples

DEFAULT RENDEZVOUS
    The default is compiled into the binary. Build with
    ALTAIR_VEGA_DEFAULT_RENDEZVOUS=<URL> to change it, or pass --room-url at
    runtime.

SHARING
    Commands that print a code or ticket can render a terminal QR code with
    --qr. Kitty-compatible terminals use the terminal graphics protocol;
    other terminals fall back to high-contrast Unicode blocks.
"#;

const ALTAIR_VEGA_PAIR_HELP: &str = r#"ALTAIR-VEGA-PAIR(1)

NAME
    altair-vega pair - create or join a short-code pairing room

DESCRIPTION
    Pairing is the universal room-building primitive. The pair command joins a
    rendezvous room and performs the same native PAKE/bootstrap handshake used
    implicitly by send, receive, and sync.

COMMANDS
    pair [--room-url <URL>] [--mode one-off|persistent] [--naked] [--qr]
        Create a pairing code and wait for another peer.

        With --naked, skip rendezvous and print a raw iroh endpoint ticket.

    pair <CODE> [--room-url <URL>] [--mode one-off|persistent]
        Join an existing pairing room.

    pair --naked <ENDPOINT_TICKET>
        Join a raw iroh endpoint ticket without using rendezvous.

    pair --inspect <CODE>
        Normalize and explain a short code without joining a room.

POSITIONALS
    CODE
        Optional short code or, with --naked, raw endpoint ticket. Short codes
        may be entered as one dash-separated value or as four space-separated
        parts. If omitted, pair hosts a new session and saves it as current
        pair state. If provided, pair joins that session and saves it as current
        pair state.

MODES
    one-off
        Exit after the pairing handshake completes.

    persistent
        Stay in the room until Ctrl+C. This is the default when no code is provided.

FLAGS
    --room-url <URL>
        Rendezvous endpoint for short-code pairing. Ignored with --naked.

    --mode one-off|persistent
        Override inferred pairing lifetime.

    --naked
        Expose or consume a raw iroh endpoint ticket instead of short-code
        rendezvous.

    --qr
        Render the printed code or ticket as a terminal QR code.

    --inspect
        Inspect CODE without starting or joining a session.

EXAMPLES
    altair-vega pair
    altair-vega pair 2048-badar-celen-votun
    altair-vega pair --inspect 2048 badar celen votun
    altair-vega pair --inspect 2048-badar-celen-votun
"#;

const ALTAIR_VEGA_TRANSFER_HELP: &str = r#"ALTAIR-VEGA-TRANSFER(1)

NAME
    altair-vega send, receive - one-off text and file transfer

DESCRIPTION
    Send and receive implicitly join a pairing room first. If send is called
    without CODE, it generates and prints a code for the receiver to enter.
    Transfers are one-off by default. If CODE or a naked ticket is omitted,
    saved pair state is reused when available.

TEXT
    altair-vega receive text [CODE]
    altair-vega send text "hello" <CODE>

NAKED TEXT TICKETS
    altair-vega receive text --naked --qr
        Listen directly on iroh and print a raw endpoint ticket.

    altair-vega send text --naked "hello" <ENDPOINT_TICKET>
        Send one message directly to the raw endpoint ticket.

FILES
    altair-vega receive file [CODE] --output-dir received-files
    altair-vega send file ./photo.jpg <CODE>

POSITIONALS
    MESSAGE
        Text body for send text.

    PATH
        File path for send file.

    CODE
        Optional short code. In --naked text send, CODE is an endpoint ticket;
        in --naked file receive, CODE is a file ticket. Raw blob tickets remain
        accepted but cannot preserve the original filename. If omitted, saved pair
        state is used when possible.

NAKED FILE TICKETS
    altair-vega send file --naked --qr ./photo.jpg
        Serve the file directly as an iroh-blobs ticket. This bypasses the
        short-code rendezvous room and exposes the underlying ticket.

    altair-vega receive file --naked <FILE_TICKET> --output-dir received-files
        Fetch a direct file ticket and preserve the original filename.

OPTIONS
    --pair-mode one-off|persistent
        Select the intended pairing lifetime. Transfer commands currently
        perform one transfer and exit.

    --room-url <URL>
        Use a specific rendezvous relay.

    --state-dir <DIR>
        Store resumable native file-transfer state.

    --naked
        Bypass short-code rendezvous and use the raw iroh ticket for the
        operation.

    --qr
        Render the printed code or naked ticket as a terminal QR code.
"#;

const ALTAIR_VEGA_SYNC_HELP: &str = r#"ALTAIR-VEGA-SYNC(1)

NAME
    altair-vega sync - persistent native folder synchronization

DESCRIPTION
    Sync uses a short-code room to exchange the internal iroh-docs ticket, then
    keeps watching and publishing folder changes. Sync commands default to a
    persistent pairing mode. If CODE or a naked docs ticket is omitted, saved
    pair state is reused when available.

COMMANDS
    sync <FOLDER> [KEY]
        Infer the role from the presence of KEY. Without KEY, publish FOLDER
        as the host and print a code. With KEY, follow that hosted sync session
        read-only. Add --join to explicitly publish local changes back.

        Without KEY, publish a folder and print the code followers should use.
        With KEY, receive hosted changes without publishing local edits back.
        With --join and KEY, use the experimental bidirectional mode that also
        publishes local changes.

        With --naked and no KEY, print the underlying iroh-docs ticket for
        direct use without short-code rendezvous. With --naked and KEY, treat
        KEY as a raw iroh-docs ticket.

        Hosts reuse their persistent docs document per folder/state directory and
        refresh the live ticket for the current endpoint on restart. Short-code
        followers receive the refreshed ticket through rendezvous.

POSITIONALS
    FOLDER
        Local folder to publish, follow, or join.

    CODE
        Optional short code. With --naked, CODE is a raw iroh-docs ticket. If
        omitted, saved pair state is used when possible.

FLAGS
    --room-url <URL>
        Rendezvous endpoint for short-code sync. Ignored with --naked.

    --pair-mode one-off|persistent
        Pairing lifetime. User-facing sync commands default to persistent.

    --naked
        Bypass short-code rendezvous and expose or consume a raw docs ticket.

    --join
        With the inferred sync form, use bidirectional join instead of read-only
        follow. This is explicit because it publishes local changes.

    --qr
        Render printed host code or docs ticket as a terminal QR code.

    --state-dir <DIR>
        Directory for sync docs state and merge-base manifests.

    --wait-ms <MS>
        Initial wait for remote docs state on follow/join.

    --interval-ms <MS>
        Poll/watch interval for sync publish and apply loops.

STATE
    Use --state-dir to choose where sync manifests and docs state are stored.
    Under disposable runtime launchers, default state is placed under the
    runtime root when possible.

    Follow and join docs nodes and base manifests are scoped by docs ticket, so
    multiple followed peers can share one state root without overwriting each
    other's stores or merge bases. Host publish state is scoped by folder path.

EXAMPLES
    altair-vega sync ./project
    altair-vega sync ./project-copy 2048-badar-celen-votun
    altair-vega sync --join ./project-copy 2048-badar-celen-votun
"#;

const ALTAIR_VEGA_SERVE_HELP: &str = r#"ALTAIR-VEGA-SERVE(1)

NAME
    altair-vega serve - bridge services for browser and native peers

DESCRIPTION
    Serve commands keep a native process online for browser/native interop and
    bridge-style workflows.

COMMANDS
    serve browser-peer <CODE> [--room-url <URL>] [--output-dir <DIR>]
        Join a browser rendezvous room as a native peer.

POSITIONALS
    CODE
        Short code for the browser rendezvous room.

FLAGS
    --room-url <URL>
        Rendezvous endpoint used by the browser page.

    --output-dir <DIR>
        Directory where browser-originated downloads are written.

EXAMPLE
    altair-vega serve browser-peer 2048-badar-celen-votun \
        --room-url ws://127.0.0.1:8788/__altair_vega_rendezvous
"#;

const ALTAIR_VEGA_RUNTIME_HELP: &str = r#"ALTAIR-VEGA-RUNTIME(1)

NAME
    altair-vega runtime - inspect disposable runtime behavior

DESCRIPTION
    Runtime diagnostics show where launcher-managed temporary state is resolved.
    This is useful when running the no-install startup scripts.

COMMANDS
    runtime inspect [--state-name <NAME>]

FLAGS
    --state-name <NAME>
        Default state directory name to resolve under the runtime root.

ENVIRONMENT
    ALTAIR_VEGA_RUNTIME_ROOT
        Runtime-owned root for default state paths.

    ALTAIR_VEGA_KEEP_RUNTIME
        Keep temporary runtime files for debugging when set.
"#;

const ALTAIR_VEGA_EXAMPLES_HELP: &str = r#"ALTAIR-VEGA-EXAMPLES(1)

TEXT TRANSFER
    receiver$ altair-vega receive text 2048-badar-celen-votun
    sender$   altair-vega send text "hello" 2048-badar-celen-votun

FILE TRANSFER
    receiver$ altair-vega receive file 2048-badar-celen-votun --output-dir ./inbox
    sender$   altair-vega send file ./archive.zip 2048-badar-celen-votun

FOLDER SYNC
    host$     altair-vega sync ./source
    follower$ altair-vega sync ./copy 2048-badar-celen-votun

PAIRING ONLY
    host$     altair-vega pair
    peer$     altair-vega pair 2048-badar-celen-votun

NAKED DIRECT ROUTES
    receiver$ altair-vega receive text --naked --qr
    sender$   altair-vega send text --naked "hello" <ENDPOINT_TICKET>
    host$     altair-vega sync --naked --qr ./source
    follower$ altair-vega sync --naked ./copy <DOCS_TICKET>

NON-DEFAULT RENDEZVOUS
    altair-vega send text --room-url ws://127.0.0.1:8788/__altair_vega_rendezvous "hello" <CODE>
"#;

async fn run_pair_room(
    code: ShortCode,
    room_url: String,
    mode: PairMode,
    qr: bool,
    peer_type: &str,
    label: &str,
) -> Result<()> {
    print_share_value("code", &code.to_string(), qr)?;
    println!("rendezvous: {room_url}");
    println!("pair mode: {mode}");
    println!("waiting for peer...");
    let peer = establish_native_control_peer(code, &room_url, peer_type, label).await?;
    println!(
        "paired with: {}",
        peer.remote_bundle.device_label.as_deref().unwrap_or("peer")
    );
    if matches!(mode, PairMode::Persistent) {
        println!("press Ctrl+C to stop");
        tokio::signal::ctrl_c().await?;
    }
    peer.endpoint.close().await;
    Ok(())
}

fn print_share_value(label: &str, value: &str, qr: bool) -> Result<()> {
    println!("{label}: {value}");
    if qr {
        print_qr(value)?;
    }
    Ok(())
}

fn print_qr(value: &str) -> Result<()> {
    let code = QrCode::new(value.as_bytes()).context("encode QR code")?;
    if std::env::var_os("KITTY_WINDOW_ID").is_some() {
        print_kitty_qr(&code)?;
        return Ok(());
    }
    let image = code
        .render::<unicode::Dense1x2>()
        .quiet_zone(true)
        .module_dimensions(2, 1)
        .build();
    println!("{image}");
    Ok(())
}

fn print_kitty_qr(code: &QrCode) -> Result<()> {
    const QUIET_ZONE: usize = 4;
    const SCALE: usize = 8;
    let modules = code.width();
    let side = (modules + QUIET_ZONE * 2) * SCALE;
    let mut rgba = vec![255u8; side * side * 4];
    for y in 0..modules {
        for x in 0..modules {
            if code[(x, y)] != Color::Dark {
                continue;
            }
            let start_x = (x + QUIET_ZONE) * SCALE;
            let start_y = (y + QUIET_ZONE) * SCALE;
            for pixel_y in start_y..start_y + SCALE {
                for pixel_x in start_x..start_x + SCALE {
                    let offset = (pixel_y * side + pixel_x) * 4;
                    rgba[offset] = 0;
                    rgba[offset + 1] = 0;
                    rgba[offset + 2] = 0;
                    rgba[offset + 3] = 255;
                }
            }
        }
    }
    let encoded = general_purpose::STANDARD.encode(rgba);
    let mut chunks = encoded.as_bytes().chunks(4096).peekable();
    let mut first = true;
    while let Some(chunk) = chunks.next() {
        let more = usize::from(chunks.peek().is_some());
        let chunk = std::str::from_utf8(chunk).context("encode kitty QR chunk")?;
        if first {
            print!("\x1b_Ga=T,f=32,s={side},v={side},m={more};{chunk}\x1b\\");
            first = false;
        } else {
            print!("\x1b_Gm={more};{chunk}\x1b\\");
        }
    }
    println!();
    Ok(())
}

async fn run_pair_command(args: PairArgs) -> Result<()> {
    let code = normalize_optional_code_args(args.code)?;
    if args.inspect {
        let code = code.context("pair --inspect requires <CODE>")?;
        let code = ShortCode::from_str(&code).context("parse short code")?;
        let [first, second, third] = code.words();
        println!("normalized: {}", code.normalized());
        println!("slot: {}", code.slot());
        println!("words: {first}, {second}, {third}");
        println!("pairing identity: {}", code.pairing_identity());
        return Ok(());
    }

    let mode = args.mode.unwrap_or(match code {
        Some(_) => PairMode::OneOff,
        None => PairMode::Persistent,
    });

    if args.naked {
        match code {
            Some(ticket) => run_naked_pair_join(&ticket, mode).await,
            None => run_naked_pair_host(mode, args.qr).await,
        }
    } else {
        match code {
            Some(code) => {
                let code = ShortCode::from_str(&code).context("parse short code")?;
                save_short_pair_state(&code, &args.room_url, mode)?;
                run_pair_room(
                    code,
                    args.room_url,
                    mode,
                    false,
                    "native-pair-join",
                    "Native Pair Join",
                )
                .await
            }
            None => {
                let code = ShortCode::generate();
                save_short_pair_state(&code, &args.room_url, mode)?;
                run_pair_room(
                    code,
                    args.room_url,
                    mode,
                    args.qr,
                    "native-pair-host",
                    "Native Pair Host",
                )
                .await
            }
        }
    }
}

async fn run_naked_pair_host(mode: PairMode, qr: bool) -> Result<()> {
    let endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![CONTROL_ALPN.to_vec()])
        .bind()
        .await
        .context("bind naked pair endpoint")?;
    let ticket = EndpointTicket::new(endpoint.addr()).to_string();
    save_endpoint_pair_state(&ticket, mode)?;
    print_share_value("endpoint ticket", &ticket, qr)?;
    println!("pair mode: {mode}");
    if matches!(mode, PairMode::OneOff) {
        let incoming = endpoint
            .accept()
            .await
            .ok_or_else(|| anyhow::anyhow!("endpoint closed before naked peer joined"))?;
        let connection = incoming
            .accept()
            .context("accept naked pair connection")?
            .await
            .context("complete naked pair connection")?;
        println!("paired with: {}", connection.remote_id());
    } else {
        println!("press Ctrl+C to stop serving this naked endpoint ticket");
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                maybe_incoming = endpoint.accept() => {
                    let Some(incoming) = maybe_incoming else {
                        break;
                    };
                    match incoming.accept().context("accept naked pair connection")?.await {
                        Ok(connection) => println!("paired with: {}", connection.remote_id()),
                        Err(error) => println!("pair error: {error}"),
                    }
                }
            }
        }
    }
    endpoint.close().await;
    Ok(())
}

async fn run_naked_pair_join(ticket: &str, mode: PairMode) -> Result<()> {
    let ticket = EndpointTicket::from_str(ticket).context("parse naked endpoint ticket")?;
    save_endpoint_pair_state(&ticket.to_string(), mode)?;
    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("bind naked pair join endpoint")?;
    let connection = endpoint
        .connect(ticket.endpoint_addr().clone(), CONTROL_ALPN)
        .await
        .context("connect to naked pair endpoint")?;
    println!("paired with: {}", connection.remote_id());
    if matches!(mode, PairMode::Persistent) {
        println!("press Ctrl+C to stop");
        tokio::signal::ctrl_c().await?;
    }
    endpoint.close().await;
    Ok(())
}

async fn run_naked_send_text(ticket: &str, message: String) -> Result<()> {
    let ticket = EndpointTicket::from_str(ticket).context("parse naked endpoint ticket")?;
    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("bind naked text sender endpoint")?;
    let connection = endpoint
        .connect(ticket.endpoint_addr().clone(), CONTROL_ALPN)
        .await
        .context("connect to naked text receiver")?;
    let (mut send, _recv) = connection
        .open_bi()
        .await
        .context("open naked text stream")?;
    write_raw_control_frame(
        &mut send,
        &ControlFrame::Message(ChatMessage {
            id: unix_secs(SystemTime::now()),
            body: message.clone(),
        }),
    )
    .await?;
    send.finish().context("finish naked text stream")?;
    endpoint.close().await;
    println!("sent naked text: {message}");
    Ok(())
}

async fn run_naked_receive_text(qr: bool) -> Result<()> {
    let endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![CONTROL_ALPN.to_vec()])
        .bind()
        .await
        .context("bind naked text receiver endpoint")?;
    let ticket = EndpointTicket::new(endpoint.addr()).to_string();
    print_share_value("endpoint ticket", &ticket, qr)?;
    println!("waiting for naked text...");
    let incoming = endpoint
        .accept()
        .await
        .ok_or_else(|| anyhow::anyhow!("endpoint closed before receiving naked text"))?;
    let connection = incoming
        .accept()
        .context("accept naked text connection")?
        .await
        .context("complete naked text connection")?;
    let (_send, mut recv) = connection
        .accept_bi()
        .await
        .context("accept naked text stream")?;
    let frame = read_raw_control_frame(&mut recv)
        .await?
        .ok_or_else(|| anyhow::anyhow!("sender disconnected before sending naked text"))?;
    let ControlFrame::Message(message) = frame else {
        bail!("expected naked text message frame");
    };
    println!("{}", message.body);
    endpoint.close().await;
    Ok(())
}

async fn write_raw_control_frame(
    send: &mut iroh::endpoint::SendStream,
    frame: &ControlFrame,
) -> Result<()> {
    let payload = altair_vega::control::encode_frame(frame)?;
    let len = u32::try_from(payload.len()).context("frame length overflow")?;
    send.write_all(&len.to_be_bytes())
        .await
        .context("write naked control frame length")?;
    send.write_all(&payload)
        .await
        .context("write naked control frame payload")?;
    Ok(())
}

async fn read_raw_control_frame(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<Option<ControlFrame>> {
    let mut len_buf = [0u8; 4];
    if recv.read_exact(&mut len_buf).await.is_err() {
        return Ok(None);
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    ensure!(len <= 64 * 1024, "naked control frame exceeds max size");
    let mut payload = vec![0u8; len];
    recv.read_exact(&mut payload)
        .await
        .context("read naked control frame payload")?;
    Ok(Some(altair_vega::control::decode_frame(&payload)?))
}

async fn run_native_send_text(code: ShortCode, room_url: String, message: String) -> Result<()> {
    println!("waiting for text receiver on code: {code}");
    let peer =
        establish_native_control_peer(code, &room_url, "native-send-text", "Native Text Sender")
            .await?;
    let mut session = ControlSession::connect(
        &peer.endpoint,
        &peer.pairing,
        &peer.local_bundle,
        &peer.remote_bundle,
    )
    .await
    .context("connect text control session")?;
    session.send_message(message.clone()).await?;
    session.finish_sending()?;
    session.wait_for_send_completion().await?;
    peer.endpoint.close().await;
    println!("sent text: {message}");
    Ok(())
}

async fn run_native_receive_text(code: ShortCode, room_url: String) -> Result<()> {
    println!("waiting for text sender on code: {code}");
    println!("leave this running, then run send text with the same code on the sender");
    let peer = establish_native_control_peer(
        code.clone(),
        &room_url,
        "native-receive-text",
        "Native Text Receiver",
    )
    .await?;
    let incoming = peer
        .endpoint
        .accept()
        .await
        .ok_or_else(|| anyhow::anyhow!("endpoint closed before receiving text"))?;
    let connection = incoming
        .accept()
        .context("accept text control connection")?
        .await
        .context("complete text control connection")?;
    let mut session = ControlSession::accept(
        connection,
        &peer.pairing,
        &peer.local_bundle,
        &peer.remote_bundle,
    )
    .await
    .context("accept text control session")?;
    let message = session
        .receive_message()
        .await?
        .ok_or_else(|| anyhow::anyhow!("sender disconnected before sending text"))?;
    println!("{}", message.body);
    session.finish_sending()?;
    session.wait_for_send_completion().await?;
    peer.endpoint.close().await;
    Ok(())
}

async fn run_native_send_file(
    code: ShortCode,
    room_url: String,
    path: PathBuf,
    state_dir: Option<PathBuf>,
) -> Result<()> {
    println!("waiting for file receiver on code: {code}");
    let peer =
        establish_native_control_peer(code, &room_url, "native-send-file", "Native File Sender")
            .await?;
    let descriptor = file_descriptor_for_path(&path)?;
    let transfer_id = u64::from_be_bytes(peer.local_bundle.session_nonce[..8].try_into().unwrap());
    let provider_dir = resolve_runtime_state_dir(state_dir, ".altair-send-file")
        .join("sender")
        .join(hash_to_hex(descriptor.hash));
    let mut session = ControlSession::connect(
        &peer.endpoint,
        &peer.pairing,
        &peer.local_bundle,
        &peer.remote_bundle,
    )
    .await
    .context("connect file control session")?;
    session
        .send_frame(ControlFrame::FileOffer(FileOffer {
            transfer_id,
            descriptor: descriptor.clone(),
            transport: FileTransport::NativeBlob,
        }))
        .await?;
    let response = session
        .receive_frame()
        .await?
        .ok_or_else(|| anyhow::anyhow!("receiver disconnected before accepting file"))?;
    let ControlFrame::FileResponse(response) = response else {
        bail!("expected file response from receiver");
    };
    ensure!(
        response.transfer_id == transfer_id,
        "file response transfer id mismatch"
    );
    ensure!(
        response.accepted,
        "receiver rejected file: {}",
        response.reason.unwrap_or_default()
    );

    if response
        .resume
        .as_ref()
        .is_none_or(|resume| resume.local_bytes < descriptor.size_bytes)
    {
        let provider = spawn_native_blob_provider(&path, &provider_dir).await?;
        session
            .send_frame(ControlFrame::FileTicket(FileTicket {
                transfer_id,
                ticket: provider.ticket.to_string(),
            }))
            .await?;
        let terminal = session
            .receive_frame()
            .await?
            .ok_or_else(|| anyhow::anyhow!("receiver disconnected before completing file"))?;
        provider.shutdown().await?;
        match terminal {
            ControlFrame::FileProgress(progress)
                if progress.phase == FileProgressPhase::Completed =>
            {
                println!(
                    "sent file: {} ({} bytes)",
                    descriptor.name, progress.bytes_complete
                );
            }
            ControlFrame::FileCancel { reason, .. } => bail!("receiver cancelled file: {reason}"),
            other => bail!("unexpected file terminal frame: {other:?}"),
        }
    } else {
        println!("receiver already has file: {}", descriptor.name);
    }

    session.finish_sending()?;
    session.wait_for_send_completion().await?;
    peer.endpoint.close().await;
    Ok(())
}

async fn run_naked_send_file(path: PathBuf, state_dir: Option<PathBuf>, qr: bool) -> Result<()> {
    let descriptor = file_descriptor_for_path(&path)?;
    let provider_dir = resolve_runtime_state_dir(state_dir, ".altair-naked-send-file")
        .join(hash_to_hex(descriptor.hash));
    let provider = spawn_native_blob_provider(&path, &provider_dir).await?;
    let blob_ticket = provider.ticket.to_string();
    let file_ticket = encode_naked_file_ticket(&blob_ticket, &descriptor.name)?;
    save_blob_pair_state(&blob_ticket, &file_ticket, PairMode::Persistent)?;
    print_share_value("file ticket", &file_ticket, qr)?;
    println!("blob ticket: {blob_ticket}");
    println!("file: {}", path.display());
    println!("bytes: {}", descriptor.size_bytes);
    println!("press Ctrl+C to stop serving this naked ticket");
    tokio::signal::ctrl_c().await?;
    provider.shutdown().await?;
    Ok(())
}

async fn run_native_receive_file(
    code: ShortCode,
    room_url: String,
    output_dir: PathBuf,
    state_dir: Option<PathBuf>,
) -> Result<()> {
    println!("waiting for file sender on code: {code}");
    println!("leave this running, then run send file with the same code on the sender");
    let peer = establish_native_control_peer(
        code.clone(),
        &room_url,
        "native-receive-file",
        "Native File Receiver",
    )
    .await?;
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("create output dir {}", output_dir.display()))?;
    let incoming = peer
        .endpoint
        .accept()
        .await
        .ok_or_else(|| anyhow::anyhow!("endpoint closed before receiving file"))?;
    let connection = incoming
        .accept()
        .context("accept file control connection")?
        .await
        .context("complete file control connection")?;
    let mut session = ControlSession::accept(
        connection,
        &peer.pairing,
        &peer.local_bundle,
        &peer.remote_bundle,
    )
    .await
    .context("accept file control session")?;
    let offer = session
        .receive_frame()
        .await?
        .ok_or_else(|| anyhow::anyhow!("sender disconnected before offering file"))?;
    let ControlFrame::FileOffer(offer) = offer else {
        bail!("expected file offer from sender");
    };
    ensure!(
        offer.transport == FileTransport::NativeBlob,
        "native receiver only accepts native blob transfers"
    );
    let store_dir = resolve_runtime_state_dir(state_dir, ".altair-receive-file")
        .join("receiver")
        .join(hash_to_hex(offer.descriptor.hash));
    fs::create_dir_all(&store_dir)
        .with_context(|| format!("create file receive state {}", store_dir.display()))?;
    let (local_bytes, complete_before) =
        inspect_native_store(&store_dir, &offer.descriptor).await?;
    session
        .send_frame(ControlFrame::FileResponse(FileResponse {
            transfer_id: offer.transfer_id,
            accepted: true,
            reason: None,
            resume: Some(altair_vega::FileResumeInfo {
                chunk_size_bytes: offer.descriptor.chunk_size_bytes,
                local_bytes,
                missing_ranges: Vec::new(),
            }),
        }))
        .await?;

    let bytes_received = if complete_before {
        local_bytes
    } else {
        let ticket = session
            .receive_frame()
            .await?
            .ok_or_else(|| anyhow::anyhow!("sender disconnected before sending blob ticket"))?;
        let ControlFrame::FileTicket(ticket) = ticket else {
            bail!("expected native blob ticket from sender");
        };
        ensure!(
            ticket.transfer_id == offer.transfer_id,
            "blob ticket transfer id mismatch"
        );
        let ticket = BlobTicket::from_str(&ticket.ticket).context("parse native blob ticket")?;
        fetch_native_blob(&store_dir, &ticket).await?
    };

    let target = unique_output_path(&output_dir, &offer.descriptor.name);
    write_native_blob_to_file(&store_dir, &offer.descriptor, &target).await?;
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
    peer.endpoint.close().await;
    println!("received file: {}", target.display());
    Ok(())
}

async fn run_naked_receive_file(
    ticket: &str,
    output_dir: PathBuf,
    state_dir: Option<PathBuf>,
) -> Result<()> {
    let ticket = parse_naked_file_ticket(ticket)?;
    let blob_ticket =
        BlobTicket::from_str(&ticket.blob_ticket).context("parse naked blob ticket")?;
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("create output dir {}", output_dir.display()))?;
    let store_dir = resolve_runtime_state_dir(state_dir, ".altair-naked-receive-file")
        .join(blob_ticket.hash_and_format().hash.to_string());
    let bytes = fetch_native_blob(&store_dir, &blob_ticket).await?;
    let target = unique_output_path(&output_dir, &safe_ticket_file_name(&ticket.file_name));
    write_naked_blob_to_file(&store_dir, &blob_ticket, &target).await?;
    println!(
        "received naked file: {} ({} bytes)",
        target.display(),
        bytes
    );
    Ok(())
}

async fn establish_native_control_peer(
    code: ShortCode,
    room_url: &str,
    peer_type: &str,
    label: &str,
) -> Result<NativeControlPeer> {
    let local_role = local_control_role(peer_type);
    let mut endpoint_builder = Endpoint::builder(presets::N0).alpns(vec![CONTROL_ALPN.to_vec()]);
    if let Some(role) = local_role {
        endpoint_builder = endpoint_builder
            .address_lookup(MdnsAddressLookup::builder())
            .user_data_for_address_lookup(local_control_user_data(&code, role)?);
    }
    let endpoint = endpoint_builder
        .bind()
        .await
        .context("bind native control endpoint")?;
    let now = SystemTime::now();
    let ttl = Duration::from_secs(60);
    let local_bundle = IrohBootstrapBundle::new(
        EndpointTicket::new(endpoint.addr()),
        PeerCapabilities::cli(),
        Some(label.to_string()),
        unix_secs(now + ttl),
    );
    if let Some(target_role) = local_control_target_role(peer_type) {
        match wait_for_local_control_peer(
            &endpoint,
            &code,
            target_role,
            local_bundle.clone(),
            now,
            ttl,
        )
        .await
        {
            Ok(Some((pairing, remote_bundle))) => {
                println!("found native peer on local network");
                return Ok(NativeControlPeer {
                    endpoint,
                    pairing,
                    local_bundle,
                    remote_bundle,
                });
            }
            Ok(None) => {}
            Err(error) => println!("local discovery unavailable: {error}"),
        }
    }

    let mut handshake = Some(PairingHandshake::new(code.clone(), now, ttl));
    let endpoint_id = random_peer_id(peer_type);
    let url = room_url_for_code(room_url, &code, &endpoint_id, peer_type, label)?;
    let room = match connect_async(url.as_str()).await {
        Ok((ws, _)) => Some(ws.split()),
        Err(error) if local_role.is_some() || local_control_target_role(peer_type).is_some() => {
            println!("rendezvous unavailable; continuing with local discovery only: {error}");
            None
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("connect native peer to rendezvous room at {url}"));
        }
    };
    if room.is_none() {
        if let Some(target_role) = local_control_target_role(peer_type) {
            return wait_for_local_control_peer_without_rendezvous(
                endpoint,
                code,
                target_role,
                label,
            )
            .await;
        }
        return accept_local_control_peer(endpoint, code, local_bundle, now, ttl).await;
    }
    let (mut write, mut read) = room.expect("room exists after early return");
    let mut established: Option<EstablishedPairing> = None;
    let mut remote_bundle: Option<IrohBootstrapBundle> = None;
    let mut pending_bootstrap: Option<PairingIntroEnvelope> = None;
    let mut sent_pake_to = std::collections::HashSet::<String>::new();
    let mut sent_bootstrap_to = std::collections::HashSet::<String>::new();

    loop {
        if let (Some(pairing), Some(remote_bundle)) = (&established, &remote_bundle) {
            return Ok(NativeControlPeer {
                endpoint,
                pairing: pairing.clone(),
                local_bundle,
                remote_bundle: remote_bundle.clone(),
            });
        }

        let next_room_message = read.next();
        tokio::pin!(next_room_message);
        let event = tokio::select! {
            maybe_local = endpoint.accept(), if local_role.is_some() => {
                if let Some(incoming) = maybe_local {
                    match incoming.accept().context("accept local native pairing connection")?.await {
                        Ok(connection) => {
                            let (pairing, remote_bundle) = complete_local_control_responder(connection, code.clone(), &local_bundle, now, ttl).await?;
                            return Ok(NativeControlPeer { endpoint, pairing, local_bundle, remote_bundle });
                        }
                        Err(error) => {
                            println!("local pairing error: {error}");
                            continue;
                        }
                    }
                }
                continue;
            }
            message = &mut next_room_message => {
                let message = message
                    .ok_or_else(|| anyhow::anyhow!("rendezvous room closed before native pairing completed"))?
                    .context("read native rendezvous message")?;
                let Some(text) = message.to_text().ok() else {
                    continue;
                };
                let Ok(event) = serde_json::from_str::<RoomServerEvent>(text) else {
                    continue;
                };
                event
            }
        };
        match event {
            RoomServerEvent::Snapshot { peers } => {
                for peer in peers {
                    send_native_pake(
                        &mut write,
                        &mut sent_pake_to,
                        &peer.endpoint_id,
                        handshake.as_ref(),
                    )
                    .await?;
                }
            }
            RoomServerEvent::PeerJoined { endpoint_id } => {
                send_native_pake(
                    &mut write,
                    &mut sent_pake_to,
                    &endpoint_id,
                    handshake.as_ref(),
                )
                .await?;
            }
            RoomServerEvent::Relay {
                from_endpoint_id,
                payload,
            } => {
                let Ok(payload) = serde_json::from_value::<ReceivedNativePairingPayload>(payload)
                else {
                    continue;
                };
                match payload {
                    ReceivedNativePairingPayload::Pake { payload } => {
                        if established.is_none()
                            && let Some(mut pending_handshake) = handshake.take()
                        {
                            let pairing = pending_handshake
                                .finish(&payload, SystemTime::now())
                                .context("finish native short-code pairing")?
                                .clone();
                            if let Some(envelope) = pending_bootstrap.take() {
                                remote_bundle = Some(pairing.open_bootstrap(&envelope)?);
                            }
                            established = Some(pairing);
                        }
                        if let Some(pairing) = &established {
                            send_native_bootstrap(
                                &mut write,
                                &mut sent_bootstrap_to,
                                &from_endpoint_id,
                                pairing,
                                &local_bundle,
                            )
                            .await?;
                        }
                    }
                    ReceivedNativePairingPayload::Bootstrap { envelope } => {
                        if let Some(pairing) = &established {
                            remote_bundle = Some(pairing.open_bootstrap(&envelope)?);
                        } else {
                            pending_bootstrap = Some(envelope);
                        }
                    }
                }
            }
            RoomServerEvent::PeerLeft { .. } => {}
        }
    }
}

fn local_control_role(peer_type: &str) -> Option<&'static str> {
    match peer_type {
        "native-receive-text" => Some("receive-text"),
        "native-receive-file" => Some("receive-file"),
        _ => None,
    }
}

fn local_control_target_role(peer_type: &str) -> Option<&'static str> {
    match peer_type {
        "native-send-text" => Some("receive-text"),
        "native-send-file" => Some("receive-file"),
        _ => None,
    }
}

fn local_control_user_data_value(code: &ShortCode, role: &str) -> String {
    format!("altair-vega:control:{role}:{}", code.normalized())
}

fn local_control_user_data(code: &ShortCode, role: &str) -> Result<UserData> {
    UserData::try_from(local_control_user_data_value(code, role))
        .context("build local native control discovery metadata")
}

async fn wait_for_local_control_peer(
    endpoint: &Endpoint,
    code: &ShortCode,
    target_role: &str,
    local_bundle: IrohBootstrapBundle,
    now: SystemTime,
    ttl: Duration,
) -> Result<Option<(EstablishedPairing, IrohBootstrapBundle)>> {
    let mdns = MdnsAddressLookup::builder()
        .advertise(false)
        .build(endpoint.id())
        .context("start local native control discovery")?;
    endpoint
        .address_lookup()
        .context("get local native control address lookup")?
        .add(mdns.clone());
    let mut events = mdns.subscribe().await;
    let target_user_data = local_control_user_data_value(code, target_role);
    let timeout = tokio::time::sleep(Duration::from_millis(2500));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => return Ok(None),
            event = events.next() => {
                let Some(event) = event else {
                    return Ok(None);
                };
                let DiscoveryEvent::Discovered { endpoint_info, .. } = event else {
                    continue;
                };
                if endpoint_info
                    .data
                    .user_data()
                    .is_none_or(|user_data| user_data.as_ref() != target_user_data)
                {
                    continue;
                }
                match connect_local_control_peer(
                    endpoint,
                    endpoint_info.into_endpoint_addr(),
                    code.clone(),
                    &local_bundle,
                    now,
                    ttl,
                )
                .await
                {
                    Ok(pairing) => return Ok(Some(pairing)),
                    Err(error) => println!("local native peer did not complete pairing: {error}"),
                }
            }
        }
    }
}

async fn wait_for_local_control_peer_without_rendezvous(
    endpoint: Endpoint,
    code: ShortCode,
    target_role: &str,
    label: &str,
) -> Result<NativeControlPeer> {
    println!("waiting for native peer on local network");
    loop {
        let now = SystemTime::now();
        let ttl = Duration::from_secs(60);
        let local_bundle = IrohBootstrapBundle::new(
            EndpointTicket::new(endpoint.addr()),
            PeerCapabilities::cli(),
            Some(label.to_string()),
            unix_secs(now + ttl),
        );
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                bail!("stopped while waiting for local native peer");
            }
            result = wait_for_local_control_peer(
                &endpoint,
                &code,
                target_role,
                local_bundle.clone(),
                now,
                ttl,
            ) => {
                match result? {
                    Some((pairing, remote_bundle)) => {
                        println!("found native peer on local network");
                        return Ok(NativeControlPeer {
                            endpoint,
                            pairing,
                            local_bundle,
                            remote_bundle,
                        });
                    }
                    None => continue,
                }
            }
        }
    }
}

async fn connect_local_control_peer(
    endpoint: &Endpoint,
    peer: iroh::EndpointAddr,
    code: ShortCode,
    local_bundle: &IrohBootstrapBundle,
    now: SystemTime,
    ttl: Duration,
) -> Result<(EstablishedPairing, IrohBootstrapBundle)> {
    let connection = endpoint
        .connect(peer, CONTROL_ALPN)
        .await
        .context("connect to local native peer")?;
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .context("open local native pairing stream")?;
    let mut handshake = PairingHandshake::new(code, now, ttl);
    write_json_frame(
        &mut send,
        &LocalPairingInit {
            pake: handshake.outbound_pake_message().to_vec(),
        },
    )
    .await?;
    let reply: LocalPairingReply = read_json_frame(&mut recv).await?;
    let pairing = handshake.finish(&reply.pake, SystemTime::now())?.clone();
    let remote_bundle = pairing.open_bootstrap(&reply.bootstrap)?;
    write_json_frame(
        &mut send,
        &LocalPairingFinish {
            bootstrap: pairing.seal_bootstrap(local_bundle)?,
        },
    )
    .await?;
    send.finish()
        .context("finish local native pairing stream")?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(connection);
    Ok((pairing, remote_bundle))
}

async fn accept_local_control_peer(
    endpoint: Endpoint,
    code: ShortCode,
    local_bundle: IrohBootstrapBundle,
    now: SystemTime,
    ttl: Duration,
) -> Result<NativeControlPeer> {
    loop {
        let incoming = endpoint
            .accept()
            .await
            .ok_or_else(|| anyhow::anyhow!("endpoint closed before local native peer joined"))?;
        match incoming
            .accept()
            .context("accept local native pairing connection")?
            .await
        {
            Ok(connection) => {
                let (pairing, remote_bundle) =
                    complete_local_control_responder(connection, code, &local_bundle, now, ttl)
                        .await?;
                println!("found native peer on local network");
                return Ok(NativeControlPeer {
                    endpoint,
                    pairing,
                    local_bundle,
                    remote_bundle,
                });
            }
            Err(error) => println!("local pairing error: {error}"),
        }
    }
}

async fn complete_local_control_responder(
    connection: iroh::endpoint::Connection,
    code: ShortCode,
    local_bundle: &IrohBootstrapBundle,
    now: SystemTime,
    ttl: Duration,
) -> Result<(EstablishedPairing, IrohBootstrapBundle)> {
    let (mut send, mut recv) = connection
        .accept_bi()
        .await
        .context("accept local native pairing stream")?;
    let init: LocalPairingInit = read_json_frame(&mut recv).await?;
    let mut handshake = PairingHandshake::new(code, now, ttl);
    let outbound_pake = handshake.outbound_pake_message().to_vec();
    let pairing = handshake.finish(&init.pake, SystemTime::now())?.clone();
    write_json_frame(
        &mut send,
        &LocalPairingReply {
            pake: outbound_pake,
            bootstrap: pairing.seal_bootstrap(local_bundle)?,
        },
    )
    .await?;
    let finish: LocalPairingFinish = read_json_frame(&mut recv).await?;
    let remote_bundle = pairing.open_bootstrap(&finish.bootstrap)?;
    send.finish().context("finish local native pairing reply")?;
    drop(connection);
    Ok((pairing, remote_bundle))
}

async fn write_json_frame<T: Serialize>(
    send: &mut iroh::endpoint::SendStream,
    value: &T,
) -> Result<()> {
    let payload = serde_json::to_vec(value).context("serialize local native pairing frame")?;
    let len = u32::try_from(payload.len()).context("local native pairing frame length overflow")?;
    send.write_all(&len.to_be_bytes())
        .await
        .context("write local native pairing frame length")?;
    send.write_all(&payload)
        .await
        .context("write local native pairing frame payload")?;
    send.flush()
        .await
        .context("flush local native pairing frame")?;
    Ok(())
}

async fn read_json_frame<T: for<'de> Deserialize<'de>>(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<T> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .context("read local native pairing frame length")?;
    let len = u32::from_be_bytes(len_buf) as usize;
    ensure!(
        len <= 256 * 1024,
        "local native pairing frame exceeds max size"
    );
    let mut payload = vec![0u8; len];
    recv.read_exact(&mut payload)
        .await
        .context("read local native pairing frame payload")?;
    serde_json::from_slice(&payload).context("decode local native pairing frame")
}

async fn send_native_pake<S>(
    write: &mut S,
    sent_pake_to: &mut std::collections::HashSet<String>,
    peer_id: &str,
    handshake: Option<&PairingHandshake>,
) -> Result<()>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    if !sent_pake_to.insert(peer_id.to_string()) {
        return Ok(());
    }
    let Some(handshake) = handshake else {
        return Ok(());
    };
    let message = NativeRoomRelayMessage {
        kind: "relay",
        to_endpoint_id: peer_id,
        payload: NativePairingPayload::Pake {
            payload: handshake.outbound_pake_message(),
        },
    };
    write
        .send(Message::Text(serde_json::to_string(&message)?.into()))
        .await
        .context("send native PAKE through rendezvous")?;
    Ok(())
}

async fn send_native_bootstrap<S>(
    write: &mut S,
    sent_bootstrap_to: &mut std::collections::HashSet<String>,
    peer_id: &str,
    pairing: &EstablishedPairing,
    local_bundle: &IrohBootstrapBundle,
) -> Result<()>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    if !sent_bootstrap_to.insert(peer_id.to_string()) {
        return Ok(());
    }
    let message = NativeRoomRelayMessage {
        kind: "relay",
        to_endpoint_id: peer_id,
        payload: NativePairingPayload::Bootstrap {
            envelope: pairing.seal_bootstrap(local_bundle)?,
        },
    };
    write
        .send(Message::Text(serde_json::to_string(&message)?.into()))
        .await
        .context("send native bootstrap through rendezvous")?;
    Ok(())
}

fn file_descriptor_for_path(path: &Path) -> Result<FileDescriptor> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("file path does not have a UTF-8 file name"))?
        .to_string();
    let bytes = fs::read(path).with_context(|| format!("read file {}", path.display()))?;
    Ok(FileDescriptor {
        name,
        size_bytes: bytes.len() as u64,
        hash: *blake3::hash(&bytes).as_bytes(),
        chunk_size_bytes: altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES,
    })
}

async fn spawn_native_blob_provider(
    source_path: &Path,
    provider_dir: &Path,
) -> Result<NativeBlobProvider> {
    let source_path = source_path
        .canonicalize()
        .with_context(|| format!("resolve source file {}", source_path.display()))?;
    fs::create_dir_all(provider_dir)
        .with_context(|| format!("create native provider dir {}", provider_dir.display()))?;
    let store = FsStore::load(provider_dir)
        .await
        .with_context(|| format!("load native provider store {}", provider_dir.display()))?;
    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("bind native blob provider endpoint")?;
    let blobs = BlobsProtocol::new(&store, None);
    let tag = blobs.add_path(&source_path).await.with_context(|| {
        format!(
            "import file into native blob store {}",
            source_path.display()
        )
    })?;
    let router = Router::builder(endpoint.clone())
        .accept(iroh_blobs::ALPN, blobs.clone())
        .spawn();
    Ok(NativeBlobProvider {
        router,
        ticket: BlobTicket::new(endpoint.addr(), tag.hash, tag.format),
    })
}

impl NativeBlobProvider {
    async fn shutdown(self) -> Result<()> {
        self.router
            .shutdown()
            .await
            .context("shutdown native blob provider")
    }
}

async fn inspect_native_store(
    store_dir: &Path,
    descriptor: &FileDescriptor,
) -> Result<(u64, bool)> {
    fs::create_dir_all(store_dir)
        .with_context(|| format!("create native receiver store {}", store_dir.display()))?;
    let store = FsStore::load(store_dir)
        .await
        .with_context(|| format!("load native receiver store {}", store_dir.display()))?;
    let local = store
        .remote()
        .local(native_hash_and_format(descriptor))
        .await
        .context("inspect native receiver store")?;
    let result = (local.local_bytes(), local.is_complete());
    store
        .shutdown()
        .await
        .context("shutdown native receiver store")?;
    Ok(result)
}

async fn fetch_native_blob(store_dir: &Path, ticket: &BlobTicket) -> Result<u64> {
    let store = FsStore::load(store_dir)
        .await
        .with_context(|| format!("load native fetch store {}", store_dir.display()))?;
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
        store
            .remote()
            .execute_get(connection, local.missing())
            .await
            .context("fetch native blob")?;
        endpoint.close().await;
    }
    let local = store
        .remote()
        .local(ticket.hash_and_format())
        .await
        .context("inspect native store after fetch")?;
    ensure!(local.is_complete(), "native blob fetch did not complete");
    let bytes = local.local_bytes();
    store
        .shutdown()
        .await
        .context("shutdown native fetch store")?;
    Ok(bytes)
}

async fn write_native_blob_to_file(
    store_dir: &Path,
    descriptor: &FileDescriptor,
    target: &Path,
) -> Result<()> {
    let store = FsStore::load(store_dir)
        .await
        .with_context(|| format!("load native output store {}", store_dir.display()))?;
    let local = store
        .remote()
        .local(native_hash_and_format(descriptor))
        .await
        .context("inspect native output store")?;
    ensure!(local.is_complete(), "native output store is incomplete");
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output parent {}", parent.display()))?;
    }
    let mut reader = store.reader(descriptor.hash);
    let mut file = tokio::fs::File::create(target)
        .await
        .with_context(|| format!("create output file {}", target.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buf)
            .await
            .context("read native blob bytes")?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        file.write_all(&buf[..read])
            .await
            .with_context(|| format!("write output file {}", target.display()))?;
    }
    file.flush().await?;
    ensure!(
        *hasher.finalize().as_bytes() == descriptor.hash,
        "received file hash mismatch"
    );
    store
        .shutdown()
        .await
        .context("shutdown native output store")?;
    Ok(())
}

async fn write_naked_blob_to_file(
    store_dir: &Path,
    ticket: &BlobTicket,
    target: &Path,
) -> Result<()> {
    let store = FsStore::load(store_dir)
        .await
        .with_context(|| format!("load naked output store {}", store_dir.display()))?;
    let local = store
        .remote()
        .local(ticket.hash_and_format())
        .await
        .context("inspect naked output store")?;
    ensure!(local.is_complete(), "naked output store is incomplete");
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output parent {}", parent.display()))?;
    }
    let mut reader = store.reader(ticket.hash_and_format().hash);
    let mut file = tokio::fs::File::create(target)
        .await
        .with_context(|| format!("create output file {}", target.display()))?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buf)
            .await
            .context("read naked blob bytes")?;
        if read == 0 {
            break;
        }
        file.write_all(&buf[..read])
            .await
            .with_context(|| format!("write output file {}", target.display()))?;
    }
    file.flush().await?;
    store
        .shutdown()
        .await
        .context("shutdown naked output store")?;
    Ok(())
}

fn native_hash_and_format(descriptor: &FileDescriptor) -> HashAndFormat {
    HashAndFormat {
        hash: descriptor.hash.into(),
        format: BlobFormat::Raw,
    }
}

fn unique_output_path(output_dir: &Path, file_name: &str) -> PathBuf {
    let candidate = output_dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or("received");
    let extension = path.extension().and_then(|item| item.to_str());
    for index in 1u64.. {
        let name = match extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        let candidate = output_dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn hash_to_hex(hash: [u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn state_key(value: &str) -> String {
    blake3::hash(value.as_bytes()).to_hex().to_string()
}

fn print_sync_changes(changes: &[altair_vega::SyncChange]) {
    let mut seen = std::collections::BTreeSet::new();
    for change in changes {
        let verb = match change.kind {
            altair_vega::SyncChangeKind::Added => "Added",
            altair_vega::SyncChangeKind::Updated => "Updated",
            altair_vega::SyncChangeKind::Deleted => "Deleted",
        };
        if seen.insert((verb, change.path.as_str())) {
            println!("{verb} {}", change.path);
        }
    }
}

fn print_sync_plan(label: &str, plan: &altair_vega::SyncMergePlan) {
    println!(
        "{label}: {} actions, {} conflicts",
        plan.actions.len(),
        plan.conflicts.len()
    );
    for action in &plan.actions {
        match action {
            altair_vega::SyncAction::UpsertFile { path, .. } => println!("Updated {path}"),
            altair_vega::SyncAction::RenamePath {
                from_path, to_path, ..
            } => println!("Renamed {from_path} -> {to_path}"),
            altair_vega::SyncAction::DeletePath { path } => println!("Deleted {path}"),
            altair_vega::SyncAction::CreateConflictCopy { .. } => {}
        }
    }
    for conflict in &plan.conflicts {
        match &conflict.resolution {
            altair_vega::SyncConflictResolution::KeepLocal => {
                println!("Conflict: kept local {}", conflict.path)
            }
            altair_vega::SyncConflictResolution::CreateRemoteConflictCopy { conflict_path } => {
                println!(
                    "Conflict: kept local {}, wrote remote copy {conflict_path}",
                    conflict.path
                )
            }
        }
    }
}

fn unix_secs(value: SystemTime) -> u64 {
    value
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time is after unix epoch")
        .as_secs()
}

async fn run_sync_host(
    root: PathBuf,
    code: ShortCode,
    room_url: String,
    state_dir: Option<PathBuf>,
    interval_ms: u64,
    naked: bool,
    qr: bool,
) -> Result<()> {
    let state_dir = resolve_runtime_state_dir(state_dir, ".altair-sync-docs-serve");
    let local_code = if naked { None } else { Some(code.normalized()) };
    let node = sync_docs::DocsSyncNode::spawn_persistent_with_local_code(
        &state_dir,
        local_code.as_deref(),
    )
    .await?;
    let current_manifest = scan_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
        .with_context(|| format!("scan sync serve root {}", root.display()))?;
    let manifest_state_path = state_dir.join(format!(
        "host-{}-last-published-manifest.json",
        state_key(&root.display().to_string())
    ));
    let doc_state_path = host_doc_state_path(&state_dir, &root);
    let previous_manifest = if manifest_state_path.exists() {
        load_manifest_state(&manifest_state_path, "host publish")?
    } else {
        current_manifest.clone()
    };
    let (doc, result) = export_host_manifest(
        &node,
        &doc_state_path,
        &root,
        &previous_manifest,
        current_manifest.clone(),
    )
    .await?;
    if let Some(local_code) = &local_code {
        node.set_local_sync_ticket(local_code, &result.ticket)?;
    }

    if naked {
        save_docs_pair_state(&result.ticket, PairMode::Persistent)?;
        println!("root: {}", root.display());
        println!("state dir: {}", state_dir.display());
        print_share_value("ticket", &result.ticket, qr)?;
        println!(
            "note: sync host reuses its docs document and refreshes the live ticket on restart"
        );
        println!("entries: {}", result.manifest.len());
        println!("content blobs: {}", result.content_blobs);
        println!("watch interval ms: {interval_ms}");
        println!("press Ctrl+C to stop");

        persist_manifest_state(&manifest_state_path, &current_manifest)?;
        let mut published_manifest = current_manifest;
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = event_tx.send(event);
        })
        .context("create naked sync host watcher")?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .with_context(|| format!("watch naked sync host root {}", root.display()))?;
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
        let host_peer = first_doc_ticket_peer(&result.ticket)?;
        let mut remote_events = Box::pin(doc.subscribe().await?);

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                maybe_remote = remote_events.next() => {
                    if maybe_remote.is_none() {
                        println!("remote sync subscription ended; reconnecting");
                        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                        remote_events = Box::pin(doc.subscribe().await?);
                        continue;
                    }
                    if let Err(error) = reconcile_host_remote(&node, &doc, &host_peer, &root, &manifest_state_path, &mut published_manifest).await {
                        println!("remote sync error: {error}");
                    }
                }
                maybe_event = event_rx.recv() => {
                    if let Some(Ok(event)) = maybe_event && matches!(event.kind, EventKind::Access(_)) {
                        continue;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    if let Err(error) = reconcile_host_remote(&node, &doc, &host_peer, &root, &manifest_state_path, &mut published_manifest).await {
                        println!("remote sync error: {error}");
                        continue;
                    }
                    let current = scan_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                        .with_context(|| format!("rescan naked sync host root {}", root.display()))?;
                    let changes = diff_manifests(&published_manifest, &current);
                    if changes.is_empty() {
                        continue;
                    }
                    match node.publish_manifest(&doc, &root, &published_manifest, &current).await {
                        Ok((content_blobs, next_manifest)) => {
                            println!("published changes: {} content blobs: {}", changes.len(), content_blobs);
                            print_sync_changes(&changes);
                            persist_manifest_state(&manifest_state_path, &next_manifest)?;
                            published_manifest = next_manifest;
                        }
                        Err(error) => {
                            println!("publish error: {error}");
                        }
                    }
                }
                _ = interval.tick() => {
                    if let Err(error) = reconcile_host_remote(&node, &doc, &host_peer, &root, &manifest_state_path, &mut published_manifest).await {
                        println!("remote sync error: {error}");
                        continue;
                    }
                    let current = scan_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                        .with_context(|| format!("rescan naked sync host root {}", root.display()))?;
                    let changes = diff_manifests(&published_manifest, &current);
                    if changes.is_empty() {
                        continue;
                    }
                    match node.publish_manifest(&doc, &root, &published_manifest, &current).await {
                        Ok((content_blobs, next_manifest)) => {
                            println!("published changes: {} content blobs: {}", changes.len(), content_blobs);
                            print_sync_changes(&changes);
                            persist_manifest_state(&manifest_state_path, &next_manifest)?;
                            published_manifest = next_manifest;
                        }
                        Err(error) => {
                            println!("publish error: {error}");
                        }
                    }
                }
            }
        }

        node.shutdown().await?;
        return Ok(());
    }

    save_short_pair_state(&code, &room_url, PairMode::Persistent)?;

    let endpoint_id = random_peer_id("native-sync-host");
    let url = room_url_for_code(
        &room_url,
        &code,
        &endpoint_id,
        "native-sync-host",
        "Native Sync Host",
    )?;
    let (mut room_write, mut room_read) = match connect_async(url.as_str()).await {
        Ok((ws, _)) => {
            let (write, read) = ws.split();
            (Some(write), Some(read))
        }
        Err(error) => {
            println!("rendezvous unavailable; continuing with local discovery only: {error}");
            (None, None)
        }
    };

    println!("root: {}", root.display());
    println!("state dir: {}", state_dir.display());
    if naked {
        print_share_value("ticket", &result.ticket, qr)?;
    }
    print_share_value("code", &code.to_string(), qr && !naked)?;
    println!("rendezvous: {room_url}");
    println!("note: sync host reuses its docs document and refreshes the live ticket on restart");
    println!("entries: {}", result.manifest.len());
    println!("content blobs: {}", result.content_blobs);
    println!("watch interval ms: {interval_ms}");
    println!("press Ctrl+C to stop");

    persist_manifest_state(&manifest_state_path, &current_manifest)?;
    let mut published_manifest = current_manifest;
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = event_tx.send(event);
    })
    .context("create sync host watcher")?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .with_context(|| format!("watch sync host root {}", root.display()))?;
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
    let host_peer = first_doc_ticket_peer(&result.ticket)?;
    let mut remote_events = Box::pin(doc.subscribe().await?);

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            maybe_remote = remote_events.next() => {
                if maybe_remote.is_none() {
                    println!("remote sync subscription ended; reconnecting");
                    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                    remote_events = Box::pin(doc.subscribe().await?);
                    continue;
                }
                if let Err(error) = reconcile_host_remote(&node, &doc, &host_peer, &root, &manifest_state_path, &mut published_manifest).await {
                    println!("remote sync error: {error}");
                }
            }
            maybe_room = async {
                match &mut room_read {
                    Some(read) => read.next().await,
                    None => std::future::pending().await,
                }
            } => {
                let Some(message) = maybe_room else {
                    continue;
                };
                let message = message.context("read sync rendezvous message")?;
                let Some(text) = message.to_text().ok() else {
                    continue;
                };
                let Ok(event) = serde_json::from_str::<RoomServerEvent>(text) else {
                    continue;
                };
                match event {
                    RoomServerEvent::Snapshot { peers } => {
                        for peer in peers {
                            if let Some(write) = &mut room_write {
                                send_sync_ticket(write, &peer.endpoint_id, &result.ticket).await?;
                            }
                        }
                    }
                    RoomServerEvent::PeerJoined { endpoint_id } => {
                        if let Some(write) = &mut room_write {
                            send_sync_ticket(write, &endpoint_id, &result.ticket).await?;
                        }
                    }
                    RoomServerEvent::PeerLeft { .. } | RoomServerEvent::Relay { .. } => {}
                }
            }
            maybe_event = event_rx.recv() => {
                if let Some(Ok(event)) = maybe_event && matches!(event.kind, EventKind::Access(_)) {
                    continue;
                }
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                if let Err(error) = reconcile_host_remote(&node, &doc, &host_peer, &root, &manifest_state_path, &mut published_manifest).await {
                    println!("remote sync error: {error}");
                    continue;
                }
                let current = scan_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                    .with_context(|| format!("rescan sync host root {}", root.display()))?;
                let changes = diff_manifests(&published_manifest, &current);
                if changes.is_empty() {
                    continue;
                }
                match node.publish_manifest(&doc, &root, &published_manifest, &current).await {
                    Ok((content_blobs, next_manifest)) => {
                        println!("published changes: {} content blobs: {}", changes.len(), content_blobs);
                        print_sync_changes(&changes);
                        persist_manifest_state(&manifest_state_path, &next_manifest)?;
                        published_manifest = next_manifest;
                    }
                    Err(error) => {
                        println!("publish error: {error}");
                    }
                }
            }
            _ = interval.tick() => {
                if let Err(error) = reconcile_host_remote(&node, &doc, &host_peer, &root, &manifest_state_path, &mut published_manifest).await {
                    println!("remote sync error: {error}");
                    continue;
                }
                let current = scan_directory(&root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                    .with_context(|| format!("rescan sync host root {}", root.display()))?;
                let changes = diff_manifests(&published_manifest, &current);
                if changes.is_empty() {
                    continue;
                }
                match node.publish_manifest(&doc, &root, &published_manifest, &current).await {
                    Ok((content_blobs, next_manifest)) => {
                        println!("published changes: {} content blobs: {}", changes.len(), content_blobs);
                        print_sync_changes(&changes);
                        persist_manifest_state(&manifest_state_path, &next_manifest)?;
                        published_manifest = next_manifest;
                    }
                    Err(error) => {
                        println!("publish error: {error}");
                    }
                }
            }
        }
    }

    node.shutdown().await?;
    Ok(())
}

async fn resolve_sync_ticket_or_code(input: &str, room_url: &str, naked: bool) -> Result<String> {
    if naked {
        return Ok(input.to_string());
    }
    match ShortCode::from_str(input) {
        Ok(code) => wait_for_sync_ticket(&code, room_url).await,
        Err(error) if input.contains('-') => Err(error).with_context(|| {
            "parse sync short code; provide a valid short code or use --naked with a raw docs ticket"
        }),
        Err(_) => Ok(input.to_string()),
    }
}

async fn wait_for_sync_ticket(code: &ShortCode, room_url: &str) -> Result<String> {
    match wait_for_local_sync_ticket(code, Duration::from_millis(2500)).await {
        Ok(Some(ticket)) => {
            println!("found sync host on local network");
            return Ok(ticket);
        }
        Ok(None) => {}
        Err(error) => {
            println!("local discovery unavailable: {error}");
        }
    }

    let endpoint_id = random_peer_id("native-sync-peer");
    let url = room_url_for_code(
        room_url,
        code,
        &endpoint_id,
        "native-sync-peer",
        "Native Sync Peer",
    )?;
    let (ws, _) = connect_async(url.as_str())
        .await
        .with_context(|| {
            format!(
                "connect sync peer to rendezvous room at {url}; is the rendezvous server running? Use --naked with a docs ticket to sync without rendezvous"
            )
        })?;
    let (_write, mut read) = ws.split();
    println!("waiting for sync host on code: {code}");

    while let Some(message) = read.next().await {
        let message = message.context("read sync rendezvous message")?;
        let Some(text) = message.to_text().ok() else {
            continue;
        };
        let Ok(RoomServerEvent::Relay { payload, .. }) =
            serde_json::from_str::<RoomServerEvent>(text)
        else {
            continue;
        };
        let Ok(payload) = serde_json::from_value::<ReceivedSyncTicketPayload>(payload) else {
            continue;
        };
        if payload.kind == "sync-ticket" {
            return Ok(payload.ticket);
        }
    }

    anyhow::bail!("sync rendezvous room closed before receiving a sync ticket")
}

async fn wait_for_local_sync_ticket(
    code: &ShortCode,
    timeout: std::time::Duration,
) -> Result<Option<String>> {
    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("bind local sync discovery endpoint")?;
    let mdns = MdnsAddressLookup::builder()
        .advertise(false)
        .build(endpoint.id())
        .context("start local sync discovery")?;
    endpoint
        .address_lookup()
        .context("get local sync address lookup")?
        .add(mdns.clone());
    let mut events = mdns.subscribe().await;
    let target_user_data = sync_docs::local_sync_user_data_value(&code.normalized());
    let timeout = tokio::time::sleep(timeout);
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => return Ok(None),
            event = events.next() => {
                let Some(event) = event else {
                    return Ok(None);
                };
                let DiscoveryEvent::Discovered { endpoint_info, .. } = event else {
                    continue;
                };
                if endpoint_info
                    .data
                    .user_data()
                    .is_none_or(|user_data| user_data.as_ref() != target_user_data)
                {
                    continue;
                }
                match request_local_sync_ticket(&endpoint, endpoint_info.into_endpoint_addr(), code).await {
                    Ok(ticket) => return Ok(Some(ticket)),
                    Err(error) => println!("local sync host did not return a ticket: {error}"),
                }
            }
        }
    }
}

async fn request_local_sync_ticket(
    endpoint: &Endpoint,
    peer: iroh::EndpointAddr,
    code: &ShortCode,
) -> Result<String> {
    let connection = endpoint
        .connect(peer, sync_docs::LOCAL_SYNC_TICKET_ALPN)
        .await
        .context("connect to local sync host")?;
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .context("open local sync ticket stream")?;
    send.write_all(code.normalized().as_bytes())
        .await
        .context("write local sync code")?;
    send.finish().context("finish local sync ticket request")?;
    let response = recv
        .read_to_end(64 * 1024)
        .await
        .context("read local sync ticket")?;
    connection.close(0u8.into(), b"done");
    String::from_utf8(response).context("decode local sync ticket")
}

async fn send_sync_ticket<S>(write: &mut S, to_endpoint_id: &str, ticket: &str) -> Result<()>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let message = RoomRelayMessage {
        kind: "relay",
        to_endpoint_id,
        payload: SyncTicketPayload {
            kind: "sync-ticket",
            ticket,
        },
    };
    write
        .send(Message::Text(serde_json::to_string(&message)?.into()))
        .await
        .context("send sync ticket through rendezvous")?;
    Ok(())
}

fn room_url_for_code(
    room_url: &str,
    code: &ShortCode,
    endpoint_id: &str,
    peer_type: &str,
    label: &str,
) -> Result<Url> {
    let mut url = Url::parse(room_url).context("parse sync rendezvous room URL")?;
    url.query_pairs_mut()
        .append_pair("code", &code.normalized())
        .append_pair("endpointId", endpoint_id)
        .append_pair("peerType", peer_type)
        .append_pair("label", label);
    Ok(url)
}

fn random_peer_id(prefix: &str) -> String {
    let mut random = [0u8; 8];
    OsRng.fill_bytes(&mut random);
    format!("{prefix}-{:016x}", u64::from_be_bytes(random))
}

fn persist_manifest_state(
    path: &std::path::Path,
    manifest: &altair_vega::SyncManifest,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create manifest state parent {}", parent.display()))?;
    }
    let tmp = path.with_extension("json.altair-tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(manifest).context("serialize published manifest state")?,
    )
    .with_context(|| format!("write manifest state temp file {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("finalize manifest state {}", path.display()))?;
    Ok(())
}

fn host_doc_state_path(state_dir: &std::path::Path, root: &std::path::Path) -> PathBuf {
    state_dir.join(format!(
        "host-{}-doc-id.txt",
        state_key(&root.display().to_string())
    ))
}

async fn export_host_manifest(
    node: &sync_docs::DocsSyncNode,
    doc_state_path: &std::path::Path,
    root: &std::path::Path,
    previous_manifest: &altair_vega::SyncManifest,
    current_manifest: altair_vega::SyncManifest,
) -> Result<(iroh_docs::api::Doc, sync_docs::DocsExportResult)> {
    if doc_state_path.exists() {
        let doc_id = std::fs::read_to_string(doc_state_path)
            .with_context(|| format!("read host docs document id {}", doc_state_path.display()))?;
        let doc = match node.open_doc(doc_id.trim()).await {
            Ok(doc) => doc,
            Err(open_error) => {
                let ticket_path = host_ticket_state_path(doc_state_path);
                let ticket = std::fs::read_to_string(&ticket_path).with_context(|| {
                    format!(
                        "open host docs document recorded in {} failed ({open_error}); read recovery ticket {}",
                        doc_state_path.display(),
                        ticket_path.display()
                    )
                })?;
                node.import_ticket_namespace(ticket.trim()).await.with_context(|| {
                    format!(
                        "recover host docs namespace from {}; move {} aside to create a fresh sync document",
                        ticket_path.display(),
                        doc_state_path.display()
                    )
                })?
            }
        };
        let result = node
            .export_existing_manifest(&doc, root, previous_manifest, current_manifest)
            .await?;
        persist_host_ticket(&host_ticket_state_path(doc_state_path), &result.ticket)?;
        Ok((doc, result))
    } else {
        let result = node
            .export_manifest(root, previous_manifest, current_manifest)
            .await?;
        persist_host_doc_id(doc_state_path, &result.doc_id)?;
        persist_host_ticket(&host_ticket_state_path(doc_state_path), &result.ticket)?;
        let doc = node.open_doc(&result.doc_id).await?;
        Ok((doc, result))
    }
}

fn host_ticket_state_path(doc_state_path: &std::path::Path) -> PathBuf {
    doc_state_path.with_extension("ticket.txt")
}

fn persist_host_ticket(path: &std::path::Path, ticket: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create host docs ticket parent {}", parent.display()))?;
    }
    std::fs::write(path, ticket)
        .with_context(|| format!("write host docs ticket {}", path.display()))
}

fn first_doc_ticket_peer(ticket: &str) -> Result<iroh::EndpointAddr> {
    let ticket = iroh_docs::DocTicket::from_str(ticket).context("parse docs ticket")?;
    ticket
        .nodes
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("docs ticket did not include any peers"))
}

fn persist_host_doc_id(path: &std::path::Path, doc_id: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create host docs document id parent {}", parent.display()))?;
    }
    std::fs::write(path, doc_id)
        .with_context(|| format!("write host docs document id {}", path.display()))
}

fn load_manifest_state(path: &std::path::Path, label: &str) -> Result<altair_vega::SyncManifest> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("read {label} manifest state {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "deserialize {label} manifest state {}; the state file may be corrupted or from an incompatible version. Move this file aside to force a fresh sync base.",
            path.display()
        )
    })
}

async fn reconcile_host_remote(
    node: &sync_docs::DocsSyncNode,
    doc: &iroh_docs::api::Doc,
    fallback_peer: &iroh::EndpointAddr,
    root: &std::path::Path,
    manifest_state_path: &std::path::Path,
    published_manifest: &mut altair_vega::SyncManifest,
) -> Result<()> {
    let remote_manifest = node.read_doc_manifest(doc).await?;
    if manifests_state_eq(published_manifest, &remote_manifest) {
        return Ok(());
    }

    let mut peer = fallback_peer.clone();
    for ticket in node.read_peer_tickets(doc).await.unwrap_or_default() {
        let Ok(candidate) = first_doc_ticket_peer(&ticket) else {
            continue;
        };
        if candidate != *fallback_peer {
            peer = candidate;
            break;
        }
    }

    let plan = node
        .apply_remote_manifest(peer, published_manifest, root, &remote_manifest)
        .await?;
    if !plan.actions.is_empty() || !plan.conflicts.is_empty() {
        print_sync_plan("applied remote", &plan);
    }
    persist_manifest_state(manifest_state_path, &remote_manifest)?;
    *published_manifest = remote_manifest;
    Ok(())
}

async fn reconcile_join_remote(
    node: &sync_docs::DocsSyncNode,
    doc: &iroh_docs::api::Doc,
    peer: &iroh::EndpointAddr,
    local: &std::path::Path,
    sync_state_path: &std::path::Path,
    base_manifest: &mut altair_vega::SyncManifest,
    last_published_manifest: &mut Option<altair_vega::SyncManifest>,
) -> Result<()> {
    let remote_manifest = node.read_doc_manifest(doc).await?;
    if manifests_state_eq(base_manifest, &remote_manifest) {
        return Ok(());
    }

    let plan = node
        .apply_remote_manifest(peer.clone(), base_manifest, local, &remote_manifest)
        .await?;
    if !plan.actions.is_empty() || !plan.conflicts.is_empty() {
        print_sync_plan("applied remote", &plan);
    }
    persist_manifest_state(sync_state_path, &remote_manifest)?;
    *base_manifest = remote_manifest;
    if let Some(last) = last_published_manifest
        && manifests_state_eq(last, base_manifest)
    {
        *last_published_manifest = None;
    }
    Ok(())
}

async fn publish_join_local_changes(
    node: &sync_docs::DocsSyncNode,
    doc: &iroh_docs::api::Doc,
    local: &std::path::Path,
    base_manifest: &altair_vega::SyncManifest,
    last_published_manifest: &mut Option<altair_vega::SyncManifest>,
) -> Result<()> {
    let current_local = scan_directory(local, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
        .with_context(|| format!("scan docs join local root {}", local.display()))?;
    let local_changes = diff_manifests(base_manifest, &current_local);
    if local_changes.is_empty() {
        return Ok(());
    }

    let proposed_manifest = altair_vega::with_tombstones(
        base_manifest,
        &current_local,
        altair_vega::unix_time_now_ms(),
    );
    let skip_publish = last_published_manifest
        .as_ref()
        .is_some_and(|last| manifests_state_eq(last, &proposed_manifest));
    if skip_publish {
        return Ok(());
    }

    match node
        .publish_manifest(doc, local, base_manifest, &current_local)
        .await
    {
        Ok((content_blobs, published_manifest)) => {
            println!(
                "published local changes: {} content blobs: {}",
                local_changes.len(),
                content_blobs
            );
            for change in &local_changes {
                println!("local {:?} {}", change.kind, change.path);
            }
            *last_published_manifest = Some(published_manifest);
        }
        Err(error) => {
            println!("local publish error: {error}");
        }
    }
    Ok(())
}

async fn wait_for_remote_manifest(
    node: &sync_docs::DocsSyncNode,
    doc: &iroh_docs::api::Doc,
    wait_ms: u64,
    require_non_empty: bool,
) -> Result<altair_vega::SyncManifest> {
    let attempts = if require_non_empty { 10 } else { 1 };
    let delay = std::time::Duration::from_millis(wait_ms.max(250));
    let mut last = altair_vega::SyncManifest::default();
    for attempt in 0..attempts {
        let manifest = node.read_doc_manifest(doc).await?;
        if !require_non_empty || !manifest.is_empty() {
            return Ok(manifest);
        }
        last = manifest;
        if attempt + 1 < attempts {
            tokio::time::sleep(delay).await;
        }
    }
    Ok(last)
}
