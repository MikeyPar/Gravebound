use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "tools_content",
    version,
    about = "Gravebound content and simulation tools"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print pinned foundation diagnostics.
    Doctor,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    match Cli::parse().command {
        Command::Doctor => {
            info!(
                sim_hz = sim_core::TICKS_PER_SECOND,
                schema_version = content_schema::SCHEMA_VERSION,
                loader_schema_version = sim_content::supported_schema_version(),
                "Gravebound GB-M00 foundation is available"
            );
        }
    }
}
