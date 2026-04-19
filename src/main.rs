use altair_vega::{ShortCode, run_local_pairing_probe};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
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
    }

    Ok(())
}
