use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "server_app", about = "Gravebound authoritative server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate the M02 server/runtime and handshake-transport boundaries.
    Doctor,
    /// Run the local M02 QUIC playtest server until Ctrl+C.
    Serve {
        /// QUIC listen address. Loopback is the safe default for local playtests.
        #[arg(long, default_value = "127.0.0.1:50000")]
        bind: SocketAddr,
        /// Validated content package root.
        #[arg(long, default_value = "content")]
        content_root: PathBuf,
        /// DER certificate written for local native clients.
        #[arg(long, default_value = "target/gravebound-local/server-cert.der")]
        certificate_out: PathBuf,
    },
    /// Run the wipeable Core identity server without M02 combat admission.
    ServeCoreIdentity {
        #[arg(long, default_value = "127.0.0.1:50001")]
        bind: SocketAddr,
        #[arg(long, default_value = "content")]
        content_root: PathBuf,
        #[arg(long, default_value = "target/gravebound-core-dev/server-cert.der")]
        certificate_out: PathBuf,
        /// Atomically published bound address for launchers that cannot parse process logs.
        #[arg(long)]
        readiness_out: Option<PathBuf>,
    },
    /// Run the previous process-local Core identity endpoint for explicit regression testing.
    ServeCoreIdentityEphemeral {
        #[arg(long, default_value = "127.0.0.1:50002")]
        bind: SocketAddr,
        #[arg(long, default_value = "content")]
        content_root: PathBuf,
        #[arg(long, default_value = "target/gravebound-core-dev/ephemeral-cert.der")]
        certificate_out: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();
    run_command(Cli::parse().command.unwrap_or_else(default_serve_command)).await
}

async fn run_command(command: Command) -> Result<()> {
    match command {
        Command::Doctor => {
            let report = server_app::run_doctor().await?;
            info!(
                protocol_major = report.protocol.major,
                protocol_minor = report.protocol.minor,
                simulation_hz = report.simulation_hz,
                snapshot_hz = report.snapshot_hz,
                database_enabled = report.database_enabled,
                transport_enabled = report.transport_enabled,
                instance_scheduler_enabled = report.instance_scheduler_enabled,
                "GB-M02 server foundation is valid"
            );
        }
        Command::Serve {
            bind,
            content_root,
            certificate_out,
        } => {
            serve_local(bind, content_root, certificate_out).await?;
        }
        Command::ServeCoreIdentity {
            bind,
            content_root,
            certificate_out,
            readiness_out,
        } => {
            serve_core_identity_persistent(bind, content_root, certificate_out, readiness_out)
                .await?;
        }
        Command::ServeCoreIdentityEphemeral {
            bind,
            content_root,
            certificate_out,
        } => {
            serve_core_identity_ephemeral(bind, content_root, certificate_out).await?;
        }
    }
    Ok(())
}

async fn serve_local(
    bind: SocketAddr,
    content_root: PathBuf,
    certificate_out: PathBuf,
) -> Result<()> {
    let server = server_app::BoundLocalServer::bind(server_app::LocalServerConfig {
        bind_address: bind,
        content_root,
    })?;
    write_certificate(&certificate_out, server.certificate_der(), "local")?;
    info!(
        address = %server.local_address(),
        certificate = %certificate_out.display(),
        build_id = server_app::LOCAL_BUILD_ID,
        "GB-M02 local playtest server is ready"
    );
    let report = server.serve_until(shutdown_signal()).await?;
    info!(
        accepted_connections = report.accepted_connections,
        admitted_sessions = report.admitted_sessions,
        scheduler_frames = report.scheduler_frames,
        dropped_snapshots = report.dropped_snapshots,
        zero_residue = report.zero_residue,
        "GB-M02 local playtest server stopped cleanly"
    );
    Ok(())
}

async fn serve_core_identity_persistent(
    bind: SocketAddr,
    content_root: PathBuf,
    certificate_out: PathBuf,
    readiness_out: Option<PathBuf>,
) -> Result<()> {
    let persistence = persistence::PostgresPersistence::connect(
        &persistence::PersistenceConfig::from_runtime_environment()?,
    )
    .await?;
    persistence.migrate().await?;
    let readiness = persistence.readiness().await?;
    let server = server_app::BoundCoreIdentityServer::bind_persistent(
        &server_app::CoreIdentityServerConfig {
            bind_address: bind,
            content_root,
        },
        server_app::PostgresAccountRepository::new(persistence.clone()),
    )?;
    write_certificate(&certificate_out, server.certificate_der(), "Core identity")?;
    if let Some(path) = readiness_out.as_ref() {
        publish_readiness(path, server.local_address())?;
    }
    info!(
        address = %server.local_address(),
        certificate = %certificate_out.display(),
        build_id = server_app::CORE_IDENTITY_BUILD_ID,
        content_target = server_app::CORE_IDENTITY_CONTENT_TARGET,
        schema_version = readiness.schema_version,
        namespace = readiness.namespace,
        wipeable = readiness.wipeable,
        "GB-M03-02B durable Core identity server is ready"
    );
    let report = server.serve_until(shutdown_signal()).await;
    if let Some(path) = readiness_out.as_ref() {
        let _ = std::fs::remove_file(path);
    }
    let report = report?;
    info!(
        accepted_connections = report.accepted_connections,
        rejected_connections = report.rejected_connections,
        combat_sessions_admitted = report.combat_sessions_admitted,
        completed_connection_tasks = report.completed_connection_tasks,
        failed_connection_tasks = report.failed_connection_tasks,
        remaining_connection_tasks = report.remaining_connection_tasks,
        remaining_open_connections = report.remaining_open_connections,
        zero_residue = report.zero_residue,
        persistence_enabled = report.persistence_enabled,
        "GB-M03-02B durable Core identity server stopped cleanly"
    );
    persistence.close().await;
    Ok(())
}

async fn serve_core_identity_ephemeral(
    bind: SocketAddr,
    content_root: PathBuf,
    certificate_out: PathBuf,
) -> Result<()> {
    let server =
        server_app::BoundCoreIdentityServer::bind(&server_app::CoreIdentityServerConfig {
            bind_address: bind,
            content_root,
        })?;
    write_certificate(
        &certificate_out,
        server.certificate_der(),
        "ephemeral Core identity",
    )?;
    info!(
        address = %server.local_address(),
        certificate = %certificate_out.display(),
        "explicit process-local Core identity regression server is ready"
    );
    let report = server.serve_until(shutdown_signal()).await?;
    info!(
        accepted_connections = report.accepted_connections,
        completed_connection_tasks = report.completed_connection_tasks,
        failed_connection_tasks = report.failed_connection_tasks,
        remaining_connection_tasks = report.remaining_connection_tasks,
        remaining_open_connections = report.remaining_open_connections,
        zero_residue = report.zero_residue,
        persistence_enabled = report.persistence_enabled,
        "ephemeral Core identity regression server stopped cleanly"
    );
    Ok(())
}

fn write_certificate(path: &PathBuf, bytes: &[u8], label: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create certificate directory {}",
                parent.display()
            )
        })?;
    }
    std::fs::write(path, bytes)
        .with_context(|| format!("failed to write {label} certificate {}", path.display()))
}

fn publish_readiness(path: &PathBuf, address: SocketAddr) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create readiness directory {}", parent.display())
        })?;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temporary, format!("{address}\n")).with_context(|| {
        format!(
            "failed to write Core identity readiness candidate {}",
            temporary.display()
        )
    })?;
    if path.exists() {
        std::fs::remove_file(path).with_context(|| {
            format!(
                "failed to replace stale Core identity readiness file {}",
                path.display()
            )
        })?;
    }
    std::fs::rename(&temporary, path).with_context(|| {
        format!(
            "failed to publish Core identity readiness file {}",
            path.display()
        )
    })
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to listen for Ctrl+C");
    }
}

fn default_serve_command() -> Command {
    let executable_root = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from));
    let packaged_content = executable_root
        .as_ref()
        .map(|root| root.join("content"))
        .filter(|path| path.is_dir());
    let certificate_out = executable_root.map_or_else(
        || PathBuf::from("server-cert.der"),
        |root| root.join("server-cert.der"),
    );
    Command::Serve {
        bind: "127.0.0.1:50000"
            .parse()
            .expect("default server bind address is valid"),
        content_root: packaged_content.unwrap_or_else(|| PathBuf::from("content")),
        certificate_out,
    }
}
