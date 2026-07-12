use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "server_app", about = "Gravebound authoritative server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate the M02 server/runtime and handshake-transport boundaries.
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
            let report = server_app::run_doctor().await?;
            info!(
                protocol_major = report.protocol.major,
                protocol_minor = report.protocol.minor,
                simulation_hz = report.simulation_hz,
                snapshot_hz = report.snapshot_hz,
                database_enabled = report.database_enabled,
                transport_enabled = report.transport_enabled,
                "GB-M02 server foundation is valid"
            );
        }
    }
    Ok(())
}
