//! Opt-in live adapter for the privacy-safe `GB-M01-10B` telemetry contract.

use std::{
    collections::BTreeMap,
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use bevy::prelude::*;
use sim_core::{
    BELL_PROCTOR_ID, BellProctorPhase, BellProctorStateKind, BossPhaseTelemetry, CohortEligibility,
    DamageTelemetry, DeathCauseTelemetry, DeathTelemetry, GenreFamiliarity, ItemLifecycleAction,
    ItemLifecycleTelemetry, LocalRunPhase, LocalTelemetryContext, LocalTelemetryLog,
    MetricEligibility, RestartReasonTelemetry, RestartTelemetry, TelemetryEvent,
};

use crate::{
    FixedSimulationSet, PackageDiagnostics, death::LocalDeathRuntime,
    developer_tools::DeveloperToolsState, encounter::EncounterClientState,
    enemies::EnemyLabRuntime,
};

const CONSENT_ENV: &str = "GRAVEBOUND_TELEMETRY_CONSENT";
const TESTER_ENV: &str = "GRAVEBOUND_TELEMETRY_TESTER_ID";
const SESSION_ENV: &str = "GRAVEBOUND_TELEMETRY_SESSION_ID";
const COHORT_ENV: &str = "GRAVEBOUND_TELEMETRY_COHORT";
const FAMILIARITY_ENV: &str = "GRAVEBOUND_TELEMETRY_GENRE_FAMILIARITY";
const OUTPUT_ENV: &str = "GRAVEBOUND_TELEMETRY_OUTPUT";

#[derive(Resource)]
struct PendingTelemetryConfig {
    tester_id: String,
    session_id: String,
    cohort: CohortEligibility,
    familiarity: GenreFamiliarity,
    output: PathBuf,
}

#[derive(Resource)]
struct LiveTelemetry {
    log: LocalTelemetryLog,
    output: PathBuf,
    timestamp: i64,
    run_ordinal: u32,
    run_id: String,
    boss_started: bool,
    boss_phase: Option<u8>,
    boss_defeated: bool,
    death_id: Option<String>,
    inventory: BTreeMap<u64, InventoryTelemetryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InventoryTelemetryEntry {
    content_id: String,
    equipped: bool,
}

impl LiveTelemetry {
    fn record(&mut self, event: TelemetryEvent) {
        self.timestamp = self.timestamp.saturating_add(1);
        self.log
            .record(self.timestamp, event)
            .expect("live telemetry adapter must emit schema-valid ordered events");
    }
}

pub(crate) fn configure(app: &mut App) -> Result<()> {
    let consent = env::var(CONSENT_ENV).ok();
    if consent.as_deref() != Some("1") {
        return Ok(());
    }
    let tester_id = required_env(TESTER_ENV)?;
    let session_id = required_env(SESSION_ENV)?;
    let output = PathBuf::from(required_env(OUTPUT_ENV)?);
    let cohort = match required_env(COHORT_ENV)?.as_str() {
        "eligible_blind" => CohortEligibility::EligibleBlind,
        "excluded_feature_contributor" => CohortEligibility::ExcludedFeatureContributor,
        "excluded_incomplete_consent" => CohortEligibility::ExcludedIncompleteConsent,
        other => bail!("invalid {COHORT_ENV} value `{other}`"),
    };
    let familiarity = match required_env(FAMILIARITY_ENV)?.as_str() {
        "new_to_both" => GenreFamiliarity::NewToBoth,
        "action_rpg_only" => GenreFamiliarity::ActionRpgOnly,
        "bullet_hell_only" => GenreFamiliarity::BulletHellOnly,
        "action_rpg_and_bullet_hell" => GenreFamiliarity::ActionRpgAndBulletHell,
        other => bail!("invalid {FAMILIARITY_ENV} value `{other}`"),
    };
    app.insert_resource(PendingTelemetryConfig {
        tester_id,
        session_id,
        cohort,
        familiarity,
        output,
    })
    .add_systems(Startup, start_live_telemetry)
    .add_systems(
        FixedUpdate,
        collect_live_telemetry.in_set(FixedSimulationSet::Telemetry),
    )
    .add_systems(Last, export_on_exit);
    Ok(())
}

fn required_env(name: &'static str) -> Result<String> {
    env::var(name).with_context(|| format!("{name} is required when {CONSENT_ENV}=1"))
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn start_live_telemetry(
    mut commands: Commands,
    config: Res<PendingTelemetryConfig>,
    package: Res<PackageDiagnostics>,
    developer: Res<DeveloperToolsState>,
    death: Res<LocalDeathRuntime>,
) {
    let metric_eligibility = if developer.gate_metrics_eligible() {
        MetricEligibility::Eligible
    } else {
        MetricEligibility::ExcludedDeveloperTools
    };
    let context = LocalTelemetryContext::new(
        config.tester_id.clone(),
        config.session_id.clone(),
        package.build_id.clone(),
        package.content_version.clone(),
        config.cohort,
        config.familiarity,
        metric_eligibility,
    )
    .expect("validated opt-in telemetry environment must compile");
    let timestamp = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after Unix epoch")
            .as_millis(),
    )
    .expect("UTC timestamp must fit i64");
    let run_ordinal = death.encounter().run_ordinal();
    let run_id = run_id(run_ordinal);
    let mut live = LiveTelemetry {
        log: LocalTelemetryLog::new(context),
        output: config.output.clone(),
        timestamp,
        run_ordinal,
        run_id: run_id.clone(),
        boss_started: false,
        boss_phase: None,
        boss_defeated: false,
        death_id: None,
        inventory: inventory_snapshot(death.inventory()),
    };
    live.record(TelemetryEvent::SessionStarted);
    live.record(TelemetryEvent::RunStarted { run_id });
    commands.insert_resource(live);
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn collect_live_telemetry(
    mut live: Option<ResMut<LiveTelemetry>>,
    mut runtime: ResMut<EnemyLabRuntime>,
    death: Res<LocalDeathRuntime>,
    encounter_client: Res<EncounterClientState>,
) {
    let Some(live) = live.as_deref_mut() else {
        return;
    };
    for observation in runtime.drain_damage_telemetry() {
        let event = &observation.damage;
        live.record(TelemetryEvent::DamageReceived(DamageTelemetry {
            run_id: live.run_id.clone(),
            source_id: source_id_for_pattern(&observation.pattern_id).to_owned(),
            pattern_id: observation.pattern_id,
            damage_type: damage_type_label(event.damage_type).to_owned(),
            raw_damage: event.raw_damage,
            final_damage: event.health_damage_applied,
            pre_hit_health: event.health_before,
            post_hit_health: event.health_after,
            target_state: if event.lethal { "dead" } else { "alive" }.to_owned(),
            simulation_tick: observation.tick.0,
            latency_ms: 0,
        }));
    }

    observe_boss(live, &runtime, &death, &encounter_client);
    observe_death(live, &death, &runtime);

    let current_run_ordinal = death.encounter().run_ordinal();
    let current_inventory = inventory_snapshot(death.inventory());
    if current_run_ordinal == live.run_ordinal && matches!(death.phase(), LocalRunPhase::Dead(_)) {
        record_inventory_removals(live, &current_inventory, "death_cleanup");
    }
    if current_run_ordinal != live.run_ordinal {
        record_inventory_removals(live, &current_inventory, "run_restart");
        let previous_run_id = live.run_id.clone();
        let new_run_id = run_id(current_run_ordinal);
        let death_id = live.death_id.clone();
        let reason = if death_id.is_some() {
            RestartReasonTelemetry::Death
        } else {
            RestartReasonTelemetry::BossVictory
        };
        let elapsed_ticks = death.last_restart().map_or(0, |restart| {
            u32::try_from(restart.restart_elapsed_ticks).unwrap_or(u32::MAX)
        });
        live.record(TelemetryEvent::RunRestarted(RestartTelemetry {
            previous_run_id,
            new_run_id: new_run_id.clone(),
            reason,
            death_id,
            elapsed_ticks,
            voluntarily_activated: true,
        }));
        live.run_ordinal = current_run_ordinal;
        live.run_id = new_run_id;
        live.boss_started = false;
        live.boss_phase = None;
        live.boss_defeated = false;
        live.death_id = None;
        live.inventory.clear();
    }
    record_inventory_additions(live, &current_inventory);
    live.inventory = current_inventory;
}

fn observe_boss(
    live: &mut LiveTelemetry,
    runtime: &EnemyLabRuntime,
    death: &LocalDeathRuntime,
    encounter_client: &EncounterClientState,
) {
    let Some(snapshot) = runtime.boss_snapshot() else {
        return;
    };
    if !live.boss_started {
        live.record(TelemetryEvent::BossStarted {
            run_id: live.run_id.clone(),
            boss_id: BELL_PROCTOR_ID.to_owned(),
        });
        live.boss_started = true;
    }
    let phase = boss_phase(snapshot.state);
    if let (Some(from_phase), Some(to_phase)) = (live.boss_phase, phase)
        && from_phase != to_phase
    {
        live.record(TelemetryEvent::BossPhaseChanged(BossPhaseTelemetry {
            run_id: live.run_id.clone(),
            boss_id: BELL_PROCTOR_ID.to_owned(),
            from_phase,
            to_phase,
            boss_health: snapshot.current_health,
        }));
    }
    live.boss_phase = phase.or(live.boss_phase);
    if matches!(snapshot.state, BellProctorStateKind::Defeated) && !live.boss_defeated {
        live.record(TelemetryEvent::BossDefeated {
            run_id: live.run_id.clone(),
            boss_id: BELL_PROCTOR_ID.to_owned(),
            clear_ticks: u32::try_from(
                encounter_client
                    .completion_clear_ticks()
                    .or_else(|| death.encounter().best_clear_ticks())
                    .unwrap_or_default(),
            )
            .unwrap_or(u32::MAX),
        });
        live.boss_defeated = true;
    }
}

fn observe_death(live: &mut LiveTelemetry, death: &LocalDeathRuntime, runtime: &EnemyLabRuntime) {
    let LocalRunPhase::Dead(cause) = death.phase() else {
        return;
    };
    let death_id = format!("death-{:08}", cause.death_id.get());
    if live.death_id.as_deref() == Some(death_id.as_str()) {
        return;
    }
    let lethal = &cause.lethal;
    live.record(TelemetryEvent::CharacterDied(DeathTelemetry {
        run_id: live.run_id.clone(),
        death_id: death_id.clone(),
        class_id: "class.grave_arbalist".to_owned(),
        level: 1,
        oath_id: None,
        active_bargain_ids: Vec::new(),
        lifetime_ticks: death.encounter().tick().0,
        session_duration_ticks: death.encounter().tick().0,
        killer_id: source_id_for_pattern(&lethal.pattern_id).to_owned(),
        pattern_id: lethal.pattern_id.clone(),
        damage_type: damage_type_label(lethal.damage_type).to_owned(),
        raw_damage: lethal.raw_damage,
        final_damage: lethal.final_damage,
        pre_hit_health: lethal.health_before,
        status_ids: Vec::new(),
        room_id: "arena.prototype.bell_laboratory_01".to_owned(),
        boss_phase: runtime
            .boss_snapshot()
            .and_then(|snapshot| boss_phase(snapshot.state)),
        party_size: 1,
        contribution_basis_points: 10_000,
        item_power_band: "prototype".to_owned(),
        ping_ms: 0,
        jitter_ms: 0,
        loss_basis_points: 0,
        correction_count: 0,
        recall_state: "inactive".to_owned(),
        cause: DeathCauseTelemetry::DirectHit,
    }));
    live.death_id = Some(death_id);
}

fn inventory_snapshot(
    inventory: &sim_core::PrototypeInventory,
) -> BTreeMap<u64, InventoryTelemetryEntry> {
    let mut snapshot = BTreeMap::new();
    for item in inventory.equipped().iter().flatten() {
        snapshot.insert(
            item.instance_id().get(),
            InventoryTelemetryEntry {
                content_id: item.content_id().as_str().to_owned(),
                equipped: true,
            },
        );
    }
    for stack in inventory.backpack().iter().flatten() {
        let (instance_id, content_id) = match stack {
            sim_core::InventoryStack::Equipment(item) => {
                (item.instance_id().get(), item.content_id().as_str())
            }
            sim_core::InventoryStack::RedTonic { instance_id, .. } => {
                (instance_id.get(), "consumable.red_tonic")
            }
        };
        snapshot.insert(
            instance_id,
            InventoryTelemetryEntry {
                content_id: content_id.to_owned(),
                equipped: false,
            },
        );
    }
    snapshot
}

fn record_inventory_removals(
    live: &mut LiveTelemetry,
    current: &BTreeMap<u64, InventoryTelemetryEntry>,
    reason: &str,
) {
    let removed: Vec<_> = live
        .inventory
        .iter()
        .filter(|(id, _)| !current.contains_key(id))
        .map(|(id, entry)| (*id, entry.clone()))
        .collect();
    for (id, entry) in removed {
        live.record(TelemetryEvent::ItemLifecycle(ItemLifecycleTelemetry {
            run_id: live.run_id.clone(),
            item_instance_id: item_id(id),
            item_content_id: entry.content_id,
            action: ItemLifecycleAction::Destroyed,
            reason: Some(reason.to_owned()),
        }));
    }
}

fn record_inventory_additions(
    live: &mut LiveTelemetry,
    current: &BTreeMap<u64, InventoryTelemetryEntry>,
) {
    let changed: Vec<_> = current
        .iter()
        .filter(|(id, entry)| live.inventory.get(id) != Some(*entry))
        .map(|(id, entry)| (*id, entry.clone()))
        .collect();
    for (id, entry) in changed {
        live.record(TelemetryEvent::ItemLifecycle(ItemLifecycleTelemetry {
            run_id: live.run_id.clone(),
            item_instance_id: item_id(id),
            item_content_id: entry.content_id,
            action: if entry.equipped {
                ItemLifecycleAction::Equipped
            } else {
                ItemLifecycleAction::PickedUp
            },
            reason: None,
        }));
    }
}

#[allow(clippy::needless_pass_by_value)]
fn export_on_exit(mut exits: MessageReader<AppExit>, mut live: Option<ResMut<LiveTelemetry>>) {
    if exits.read().next().is_none() {
        return;
    }
    let Some(live) = live.as_deref_mut() else {
        return;
    };
    live.record(TelemetryEvent::SessionEnded);
    let data = live
        .log
        .export_json_lines()
        .expect("validated telemetry records serialize");
    if let Some(parent) = live.output.parent() {
        fs::create_dir_all(parent).expect("telemetry output directory must be creatable");
    }
    let temporary = live.output.with_extension("partial.jsonl");
    fs::write(&temporary, data).expect("telemetry temporary export must be writable");
    fs::rename(&temporary, &live.output).expect("telemetry export must publish atomically");
}

const fn boss_phase(state: BellProctorStateKind) -> Option<u8> {
    match state {
        BellProctorStateKind::Active(phase) | BellProctorStateKind::Break { entering: phase } => {
            Some(match phase {
                BellProctorPhase::Phase1 => 1,
                BellProctorPhase::Phase2 => 2,
                BellProctorPhase::Phase3 => 3,
            })
        }
        BellProctorStateKind::Defeated => None,
    }
}

fn source_id_for_pattern(pattern_id: &str) -> &'static str {
    if pattern_id.contains("bell_proctor") {
        BELL_PROCTOR_ID
    } else if pattern_id.contains("drowned_pilgrim") {
        sim_core::DROWNED_PILGRIM_ID
    } else if pattern_id.contains("bell_reed") {
        sim_core::BELL_REED_ID
    } else {
        sim_core::CHAIN_SENTRY_ID
    }
}

const fn damage_type_label(value: sim_core::DamageType) -> &'static str {
    match value {
        sim_core::DamageType::Physical => "physical",
        sim_core::DamageType::Veil => "veil",
    }
}

fn run_id(ordinal: u32) -> String {
    format!("run-{ordinal:08}")
}

fn item_id(value: u64) -> String {
    format!("item-{value:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_phase_and_identifier_adapters_are_stable() {
        assert_eq!(
            source_id_for_pattern("pattern.prototype.bell_proctor.gap_ring"),
            BELL_PROCTOR_ID
        );
        assert_eq!(
            source_id_for_pattern("pattern.prototype.bell_reed.gap_ring"),
            sim_core::BELL_REED_ID
        );
        assert_eq!(
            boss_phase(BellProctorStateKind::Break {
                entering: BellProctorPhase::Phase3
            }),
            Some(3)
        );
        assert_eq!(run_id(2), "run-00000002");
        assert_eq!(item_id(42), "item-000000000000002a");
    }
}
