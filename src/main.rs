use altair_vega::{MessagingPeerKind, ShortCode, run_local_message_probe, run_local_pairing_probe};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::str::FromStr;

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
    }

    Ok(())
}
