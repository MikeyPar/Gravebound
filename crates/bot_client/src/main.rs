use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "bot_client", about = "Gravebound headless journey client")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate the M02 bot boundary without pretending a journey transport exists.
    Doctor,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();
    match Cli::parse().command {
        Command::Doctor => {
            let report = bot_client::run_doctor().await?;
            info!(
                protocol_major = report.protocol.major,
                protocol_minor = report.protocol.minor,
                expected_server_hz = report.expected_server_hz,
                transport_enabled = report.transport_enabled,
                journey_enabled = report.journey_enabled,
                "GB-M02 bot foundation is valid"
            );
        }
    }
    Ok(())
}
