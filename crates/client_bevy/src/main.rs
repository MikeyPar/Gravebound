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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CoreCaldusEvidenceStateArg {
    Staging,
    Introduction,
    PhaseOne,
    ChargePressure,
    FinalRings,
    VictoryExit,
    ExtractionCommitted,
    HallArrival,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CoreTransitionEvidenceStateArg {
    HallLoading,
    DungeonLoading,
    RecoverableError,
    FatalError,
    LinkLost,
    Reconnecting,
    SameStateRecovery,
    HallResolution,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CoreEquipmentEvidenceStateArg {
    Comparison,
    IconMatrix,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CoreDeathViewEvidenceStateArg {
    Summary,
    SummaryActions,
    SummaryTrace,
    MemorialList,
    MemorialDetail,
    AwaitingCommit,
    RecoverableError,
}

impl From<CoreTransitionEvidenceStateArg> for client_bevy::CoreTransitionShowcaseState {
    fn from(value: CoreTransitionEvidenceStateArg) -> Self {
        match value {
            CoreTransitionEvidenceStateArg::HallLoading => Self::HallLoading,
            CoreTransitionEvidenceStateArg::DungeonLoading => Self::DungeonLoading,
            CoreTransitionEvidenceStateArg::RecoverableError => Self::RecoverableError,
            CoreTransitionEvidenceStateArg::FatalError => Self::FatalError,
            CoreTransitionEvidenceStateArg::LinkLost => Self::LinkLost,
            CoreTransitionEvidenceStateArg::Reconnecting => Self::Reconnecting,
            CoreTransitionEvidenceStateArg::SameStateRecovery => Self::SameStateRecovery,
            CoreTransitionEvidenceStateArg::HallResolution => Self::HallResolution,
        }
    }
}

impl From<CoreDeathViewEvidenceStateArg> for client_bevy::CoreDeathViewShowcaseState {
    fn from(value: CoreDeathViewEvidenceStateArg) -> Self {
        match value {
            CoreDeathViewEvidenceStateArg::Summary => Self::Summary,
            CoreDeathViewEvidenceStateArg::SummaryActions => Self::SummaryActions,
            CoreDeathViewEvidenceStateArg::SummaryTrace => Self::SummaryTrace,
            CoreDeathViewEvidenceStateArg::MemorialList => Self::MemorialList,
            CoreDeathViewEvidenceStateArg::MemorialDetail => Self::MemorialDetail,
            CoreDeathViewEvidenceStateArg::AwaitingCommit => Self::AwaitingCommit,
            CoreDeathViewEvidenceStateArg::RecoverableError => Self::RecoverableError,
        }
    }
}

impl From<CoreCaldusEvidenceStateArg> for client_bevy::CoreCaldusShowcaseState {
    fn from(value: CoreCaldusEvidenceStateArg) -> Self {
        match value {
            CoreCaldusEvidenceStateArg::Staging => Self::Staging,
            CoreCaldusEvidenceStateArg::Introduction => Self::Introduction,
            CoreCaldusEvidenceStateArg::PhaseOne => Self::PhaseOne,
            CoreCaldusEvidenceStateArg::ChargePressure => Self::ChargePressure,
            CoreCaldusEvidenceStateArg::FinalRings => Self::FinalRings,
            CoreCaldusEvidenceStateArg::VictoryExit => Self::VictoryExit,
            CoreCaldusEvidenceStateArg::ExtractionCommitted => Self::ExtractionCommitted,
            CoreCaldusEvidenceStateArg::HallArrival => Self::HallArrival,
        }
    }
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
    /// Open the disposable GB-M03-03D fixed-room and Core 6/2 evidence surface.
    CoreEncounterShowcase {
        #[arg(long, default_value = "content")]
        content_root: PathBuf,
        #[arg(long)]
        reduced_effects: bool,
    },
    /// Open the disposable GB-M03-03E Sir Caldus encounter and Hall-return evidence surface.
    CoreCaldusShowcase {
        #[arg(long, default_value = "content")]
        content_root: PathBuf,
        #[arg(long)]
        reduced_effects: bool,
        #[arg(long, value_enum, default_value_t = CoreCaldusEvidenceStateArg::PhaseOne)]
        state: CoreCaldusEvidenceStateArg,
    },
    /// Open the disposable GB-M03-03F loading, error, and reconnect evidence surface.
    CoreTransitionShowcase {
        #[arg(long, default_value = "content")]
        content_root: PathBuf,
        #[arg(long)]
        reduced_effects: bool,
        #[arg(long, value_enum, default_value_t = CoreTransitionEvidenceStateArg::HallLoading)]
        state: CoreTransitionEvidenceStateArg,
    },
    /// Open the disposable GB-M03-04E field inventory and icon-review evidence surface.
    CoreEquipmentShowcase {
        #[arg(long, default_value = "content")]
        content_root: PathBuf,
        #[arg(long)]
        reduced_effects: bool,
        #[arg(long, value_enum, default_value_t = CoreEquipmentEvidenceStateArg::Comparison)]
        state: CoreEquipmentEvidenceStateArg,
    },
    /// Open the disposable GB-M03-04G item and Vault lifecycle inspection surface.
    CoreItemLifecycleShowcase {
        #[arg(long, default_value = "content")]
        content_root: PathBuf,
        #[arg(long)]
        reduced_effects: bool,
    },
    /// Open the disposable GB-M03-06D native death-summary and Memorial evidence surface.
    CoreDeathViewShowcase {
        #[arg(long, default_value = "content")]
        content_root: PathBuf,
        #[arg(long)]
        reduced_effects: bool,
        #[arg(long, default_value_t = 100)]
        ui_scale: u16,
        #[arg(long, value_enum, default_value_t = CoreDeathViewEvidenceStateArg::Summary)]
        state: CoreDeathViewEvidenceStateArg,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Gravebound client failed to start: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    match Cli::parse().command.unwrap_or_else(default_command) {
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
        } => run_core_identity(server, certificate, identity, content_root),
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
        Command::CoreEncounterShowcase {
            content_root,
            reduced_effects,
        } => run_encounter_showcase(content_root, reduced_effects),
        Command::CoreCaldusShowcase {
            content_root,
            reduced_effects,
            state,
        } => client_bevy::run_core_caldus_showcase(client_bevy::CoreCaldusShowcaseConfig {
            content_root,
            reduced_effects,
            state: state.into(),
        }),
        Command::CoreTransitionShowcase {
            content_root,
            reduced_effects,
            state,
        } => run_transition_showcase(content_root, reduced_effects, state),
        Command::CoreEquipmentShowcase {
            content_root,
            reduced_effects,
            state,
        } => run_equipment_showcase(content_root, reduced_effects, state),
        Command::CoreItemLifecycleShowcase {
            content_root,
            reduced_effects,
        } => run_item_lifecycle_showcase(content_root, reduced_effects),
        Command::CoreDeathViewShowcase {
            content_root,
            reduced_effects,
            ui_scale,
            state,
        } => run_death_view_showcase(content_root, reduced_effects, ui_scale, state),
    }
}

fn run_core_identity(
    server: SocketAddr,
    certificate: PathBuf,
    identity: String,
    content_root: PathBuf,
) -> anyhow::Result<()> {
    client_bevy::run_core_identity(client_bevy::CoreIdentityConfig {
        server_address: server,
        certificate_path: certificate,
        test_token: identity,
        content_root,
    })
}

fn run_encounter_showcase(content_root: PathBuf, reduced_effects: bool) -> anyhow::Result<()> {
    client_bevy::run_core_encounter_showcase(client_bevy::CoreEncounterShowcaseConfig {
        content_root,
        reduced_effects,
    })
}

fn run_item_lifecycle_showcase(content_root: PathBuf, reduced_effects: bool) -> anyhow::Result<()> {
    client_bevy::run_core_item_lifecycle_showcase(&client_bevy::CoreItemLifecycleShowcaseConfig {
        content_root,
        reduced_effects,
    })
}

fn run_death_view_showcase(
    content_root: PathBuf,
    reduced_effects: bool,
    ui_scale_percent: u16,
    state: CoreDeathViewEvidenceStateArg,
) -> anyhow::Result<()> {
    client_bevy::run_core_death_view_showcase(&client_bevy::CoreDeathViewShowcaseConfig {
        content_root,
        reduced_effects,
        ui_scale_percent,
        state: state.into(),
    })
}

fn run_equipment_showcase(
    content_root: PathBuf,
    reduced_effects: bool,
    state: CoreEquipmentEvidenceStateArg,
) -> anyhow::Result<()> {
    client_bevy::run_core_equipment_showcase(&client_bevy::CoreEquipmentShowcaseConfig {
        content_root,
        reduced_effects,
        state: match state {
            CoreEquipmentEvidenceStateArg::Comparison => {
                client_bevy::CoreEquipmentShowcaseState::Comparison
            }
            CoreEquipmentEvidenceStateArg::IconMatrix => {
                client_bevy::CoreEquipmentShowcaseState::IconMatrix
            }
        },
    })
}

fn run_transition_showcase(
    content_root: PathBuf,
    reduced_effects: bool,
    state: CoreTransitionEvidenceStateArg,
) -> anyhow::Result<()> {
    client_bevy::run_core_transition_showcase(&client_bevy::CoreTransitionShowcaseConfig {
        content_root,
        reduced_effects,
        state: state.into(),
    })
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
