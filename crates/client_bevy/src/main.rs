use std::{net::SocketAddr, path::PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CoreWorldSceneArg {
    Hall,
    Microrealm,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CoreWorldEvidenceStateArg {
    HallStageDisabled,
    MicrorealmWarning,
    MicrorealmCleared,
}

#[derive(Debug, Parser)]
#[command(name = "client_bevy", about = "Gravebound native client")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the local, single-process First Playable laboratory.
    LocalLab,
    /// Connect the native client to the nonpersistent M02 local QUIC server.
    Network {
        #[arg(long, default_value = "127.0.0.1:50000")]
        server: SocketAddr,
        #[arg(long, default_value = "target/gravebound-local/server-cert.der")]
        certificate: PathBuf,
        /// Unique local test token. It is treated as an opaque credential and never logged.
        #[arg(long)]
        player: String,
        #[arg(long, default_value = "content")]
        content_root: PathBuf,
    },
    /// Open the wipeable GB-M03 Core identity and Grave Arbalist character-select surface.
    CoreIdentity {
        #[arg(long, default_value = "127.0.0.1:50001")]
        server: SocketAddr,
        #[arg(long, default_value = "target/gravebound-core-dev/server-cert.der")]
        certificate: PathBuf,
        /// Opaque wipeable test credential. It is never displayed or logged.
        #[arg(long)]
        identity: String,
        #[arg(long, default_value = "content")]
        content_root: PathBuf,
    },
    /// Open the disposable GB-M03 Core Hall or private-microrealm graybox showcase.
    CoreWorldShowcase {
        #[arg(long, value_enum, default_value_t = CoreWorldSceneArg::Hall)]
        scene: CoreWorldSceneArg,
        #[arg(long, default_value = "content")]
        content_root: PathBuf,
        #[arg(long)]
        reduced_motion: bool,
        /// Prepare a deterministic disposable runtime state for screenshot inspection.
        #[arg(long, value_enum)]
        evidence_state: Option<CoreWorldEvidenceStateArg>,
    },
}

fn main() {
    let result = match Cli::parse().command.unwrap_or_else(default_command) {
        Command::LocalLab => client_bevy::run_local_lab(),
        Command::Network {
            server,
            certificate,
            player,
            content_root,
        } => client_bevy::run_network_playtest(client_bevy::NetworkPlayConfig {
            server_address: server,
            certificate_path: certificate,
            player_token: player,
            content_root,
        }),
        Command::CoreIdentity {
            server,
            certificate,
            identity,
            content_root,
        } => client_bevy::run_core_identity(client_bevy::CoreIdentityConfig {
            server_address: server,
            certificate_path: certificate,
            test_token: identity,
            content_root,
        }),
        Command::CoreWorldShowcase {
            scene,
            content_root,
            reduced_motion,
            evidence_state,
        } => client_bevy::run_core_world_showcase(client_bevy::CoreWorldShowcaseConfig {
            content_root,
            scene: match scene {
                CoreWorldSceneArg::Hall => client_bevy::CoreWorldShowcaseScene::Hall,
                CoreWorldSceneArg::Microrealm => client_bevy::CoreWorldShowcaseScene::Microrealm,
            },
            reduced_motion,
            evidence_state: evidence_state.map(|state| match state {
                CoreWorldEvidenceStateArg::HallStageDisabled => {
                    client_bevy::CoreWorldShowcaseEvidenceState::HallStageDisabled
                }
                CoreWorldEvidenceStateArg::MicrorealmWarning => {
                    client_bevy::CoreWorldShowcaseEvidenceState::MicrorealmWarning
                }
                CoreWorldEvidenceStateArg::MicrorealmCleared => {
                    client_bevy::CoreWorldShowcaseEvidenceState::MicrorealmCleared
                }
            }),
        }),
    };
    if let Err(error) = result {
        eprintln!("Gravebound client failed to start: {error:#}");
        std::process::exit(1);
    }
}

fn default_command() -> Command {
    let Some(root) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
    else {
        return Command::LocalLab;
    };
    let certificate = root.join("server-cert.der");
    let content_root = root.join("content");
    if certificate.is_file() && content_root.is_dir() {
        Command::Network {
            server: "127.0.0.1:50000"
                .parse()
                .expect("default client server address is valid"),
            certificate,
            player: "local-player-default".to_owned(),
            content_root,
        }
    } else {
        Command::LocalLab
    }
}
