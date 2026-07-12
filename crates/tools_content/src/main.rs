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
    /// Validate schemas, references, release combinations, assets, and localization.
    Validate {
        /// Content root containing fp, manifests, localization, and features.
        #[arg(long, default_value = "content")]
        root: PathBuf,
    },
    /// Validate the independently hashed, non-promotable Core world-flow target.
    ValidateCoreWorldFlow {
        /// Content root containing the immutable FP source and `core_dev` files.
        #[arg(long, default_value = "content")]
        root: PathBuf,
    },
    /// Validate the independently hashed, non-promotable Core progression target.
    ValidateCoreProgression {
        /// Content root containing the immutable FP source and `core_dev` files.
        #[arg(long, default_value = "content")]
        root: PathBuf,
    },
    /// Regenerate checked-in JSON Schema contracts.
    GenerateSchemas {
        /// Destination directory for generated schemas.
        #[arg(long, default_value = "schemas")]
        output: PathBuf,
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
        Command::Validate { root } => validate_content_command(&root)?,
        Command::ValidateCoreWorldFlow { root } => validate_core_world_flow_command(&root)?,
        Command::ValidateCoreProgression { root } => validate_core_progression_command(&root)?,
        Command::GenerateSchemas { output } => generate_schemas_command(&output)?,
    }

    Ok(())
}

fn validate_core_progression_command(root: &std::path::Path) -> Result<()> {
    let compiled = sim_content::load_core_development_progression(root)?;
    info!(
        target = compiled.target_name(),
        xp_cap = compiled.level_curve().xp_cap(),
        profiles = compiled.xp_profiles().len(),
        source_bindings = compiled.source_bindings().len(),
        records_blake3 = compiled.hashes().records_blake3,
        localization_blake3 = compiled.hashes().localization_blake3,
        "unpromoted Core progression target is valid"
    );
    Ok(())
}

fn validate_core_world_flow_command(root: &std::path::Path) -> Result<()> {
    let compiled = sim_content::load_core_development_world_flow(root)?;
    info!(
        target = compiled.target_name(),
        hubs = 1,
        worlds = 1,
        objects = compiled.objects().len(),
        records_blake3 = compiled.hashes().records_blake3,
        assets_blake3 = compiled.hashes().assets_blake3,
        localization_blake3 = compiled.hashes().localization_blake3,
        "unpromoted Core world-flow target is valid"
    );
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
    let stem = fixture_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("trace");
    fixture_path.with_file_name(format!("{stem}.golden.json"))
}

fn validate_content_command(root: &std::path::Path) -> Result<()> {
    let (_, report) = sim_content::load_and_validate(root)?;
    info!(
        content_version = report.content_version,
        records = report.record_count,
        features = report.feature_count,
        package_hash_blake3 = report.package_hash_blake3,
        "content package is valid"
    );
    Ok(())
}

fn generate_schemas_command(output: &std::path::Path) -> Result<()> {
    fs::create_dir_all(output)
        .with_context(|| format!("failed to create schema directory {}", output.display()))?;
    write_schema::<Vec<content_schema::ClassRecord>>(output, "classes.schema.json")?;
    write_schema::<Vec<content_schema::AbilityRecord>>(output, "abilities.schema.json")?;
    write_schema::<Vec<content_schema::EnemyRecord>>(output, "enemies.schema.json")?;
    write_schema::<Vec<content_schema::BossRecord>>(output, "bosses.schema.json")?;
    write_schema::<Vec<content_schema::PatternRecord>>(output, "patterns.schema.json")?;
    write_schema::<Vec<content_schema::ArenaRecord>>(output, "arenas.schema.json")?;
    write_schema::<Vec<content_schema::ItemRecord>>(output, "items.schema.json")?;
    write_schema::<Vec<content_schema::DropTableRecord>>(output, "drop_tables.schema.json")?;
    write_schema::<content_schema::ReleaseManifest>(output, "release_manifest.schema.json")?;
    write_schema::<content_schema::FeatureRegistry>(output, "feature_registry.schema.json")?;
    write_schema::<content_schema::AssetManifest>(output, "asset_manifest.schema.json")?;
    write_schema::<content_schema::CoreWorldFlowDevelopmentTarget>(
        output,
        "core_world_flow_target.schema.json",
    )?;
    write_schema::<content_schema::CoreWorldFlowRecords>(
        output,
        "core_world_flow_records.schema.json",
    )?;
    write_schema::<content_schema::CoreGrayboxAssetManifest>(
        output,
        "core_graybox_assets.schema.json",
    )?;
    write_schema::<content_schema::CoreWorldFlowCopyFile>(
        output,
        "core_world_flow_copy.schema.json",
    )?;
    write_schema::<content_schema::CoreProgressionDevelopmentTarget>(
        output,
        "core_progression_target.schema.json",
    )?;
    write_schema::<content_schema::CoreProgressionRecords>(
        output,
        "core_progression_records.schema.json",
    )?;
    info!(output = %output.display(), "JSON schemas generated");
    Ok(())
}

fn write_schema<T: schemars::JsonSchema>(output: &std::path::Path, name: &str) -> Result<()> {
    let schema = schemars::schema_for!(T);
    let text = format!("{}\n", serde_json::to_string_pretty(&schema)?);
    fs::write(output.join(name), text).with_context(|| format!("failed to write schema {name}"))
}
