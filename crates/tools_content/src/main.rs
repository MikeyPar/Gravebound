use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use sim_core::{TraceFixture, TraceReport, run_trace};
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
    /// Run a deterministic fixture and verify its checked-in golden report.
    Trace {
        /// JSON trace fixture to execute.
        fixture: PathBuf,
        /// Expected report. Defaults to `<fixture>.golden.json`.
        #[arg(long)]
        golden: Option<PathBuf>,
        /// Print the report without comparing a golden file.
        #[arg(long)]
        no_verify: bool,
    },
}

fn main() -> Result<()> {
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
        Command::Trace {
            fixture,
            golden,
            no_verify,
        } => run_trace_command(&fixture, golden, no_verify)?,
    }

    Ok(())
}

fn run_trace_command(
    fixture_path: &std::path::Path,
    golden: Option<PathBuf>,
    no_verify: bool,
) -> Result<()> {
    let fixture_text = fs::read_to_string(fixture_path)
        .with_context(|| format!("failed to read trace fixture {}", fixture_path.display()))?;
    let fixture: TraceFixture = serde_json::from_str(&fixture_text)
        .with_context(|| format!("invalid trace fixture {}", fixture_path.display()))?;
    let actual = run_trace(&fixture).context("deterministic trace failed")?;

    if !no_verify {
        let golden_path = golden.unwrap_or_else(|| default_golden_path(fixture_path));
        let golden_text = fs::read_to_string(&golden_path)
            .with_context(|| format!("failed to read golden report {}", golden_path.display()))?;
        let expected: TraceReport = serde_json::from_str(&golden_text)
            .with_context(|| format!("invalid golden report {}", golden_path.display()))?;
        if actual != expected {
            bail!(
                "deterministic trace mismatch for {}; rerun with --no-verify to inspect actual output",
                fixture_path.display()
            );
        }
        info!(
            fixture = %fixture_path.display(),
            golden = %golden_path.display(),
            hashes = actual.tick_hashes.len(),
            "deterministic trace matches golden report"
        );
    }

    println!("{}", serde_json::to_string_pretty(&actual)?);
    Ok(())
}

fn default_golden_path(fixture_path: &std::path::Path) -> PathBuf {
    let mut name = fixture_path.as_os_str().to_os_string();
    name.push(".golden.json");
    PathBuf::from(name)
}
