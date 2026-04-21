use altair_vega::{
    FileProbeConfig, FileProbeMode, MessagingPeerKind, ShortCode, run_local_file_probe,
    run_local_file_probe_with_config, run_local_message_probe, run_local_native_resume_probe,
    run_local_pairing_probe,
};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::{path::PathBuf, str::FromStr};

mod browser_peer;

#[derive(Debug, Parser)]
#[command(name = "altair-vega")]
#[command(about = "Early Altair Vega pairing and bootstrap tools")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Code {
        #[command(subcommand)]
        command: CodeCommand,
    },
    Pairing {
        #[command(subcommand)]
        command: PairingCommand,
    },
    Message {
        #[command(subcommand)]
        command: MessageCommand,
    },
    File {
        #[command(subcommand)]
        command: FileCommand,
    },
    BrowserPeer {
        #[command(subcommand)]
        command: BrowserPeerCommand,
    },
}

#[derive(Debug, Subcommand)]
enum CodeCommand {
    Generate,
    Inspect { code: String },
}

#[derive(Debug, Subcommand)]
enum PairingCommand {
    Demo { code: Option<String> },
}

#[derive(Debug, Subcommand)]
enum MessageCommand {
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
    Serve {
        code: String,
        #[arg(long, default_value = "ws://127.0.0.1:5173/__altair_vega_rendezvous")]
        room_url: String,
        #[arg(long, default_value = "browser-peer-downloads")]
        output_dir: PathBuf,
    },
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
        Command::Code { command } => match command {
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
        Command::Pairing { command } => match command {
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
        Command::Message { command } => match command {
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
        Command::File { command } => match command {
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
        Command::BrowserPeer { command } => match command {
            BrowserPeerCommand::Serve {
                code,
                room_url,
                output_dir,
            } => {
                let code = ShortCode::from_str(&code).context("parse short code")?;
                browser_peer::run_browser_peer(code.normalized(), room_url, output_dir).await?;
            }
        },
    }

    Ok(())
}
