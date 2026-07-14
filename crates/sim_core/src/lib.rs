//! Renderer-independent deterministic simulation primitives.
//!
//! This crate is the sole owner of authoritative time, entity allocation, random streams, and
//! canonical foundation-state hashing. It intentionally has no Bevy or platform dependency.

mod ability;
mod arena;
mod authority;
mod bargain;
mod bargain_offer;
mod boss;
mod boss_encounter;
mod clock;
mod collision;
mod combat;
mod consumable;
mod core_counterplay;
mod core_enemy;
mod core_microrealm;
mod damage;
mod death;
mod debug_state;
mod dungeon_room;
mod encounter;
mod enemy;
mod enemy_health;
mod enemy_lab;
mod entity;
mod hostile;
mod inventory;
mod item_lifecycle;
mod movement;
mod normal_wave;
mod oath;
mod pattern;
mod performance;
mod production_item;
mod progression;
mod readability;
mod rng;
mod scene_interaction;
mod shared_authority;
mod telemetry;
mod trace;
mod weapon;
mod world_scene;

pub use ability::{
    AbilityDefinitionError, BASIS_POINTS_PER_ONE, GraveMarkDefinition,
    GraveMarkDefinitionParameters, IntentMathError, SlipstepDefinition,
    SlipstepDefinitionParameters, StillnessDefinition, StillnessDefinitionParameters,
};
pub use arena::{
    ArenaAnchor, ArenaGeometry, ArenaGeometryError, MILLI_TILES_PER_TILE, TilePoint, TileRectangle,
};
pub use authority::{
    AuthoritativeArena, AuthorityDefinitions, AuthorityEntityKind, AuthorityEntitySnapshot,
    AuthorityError, AuthorityInput, AuthorityPhase, AuthorityRecallCommit, AuthorityStep,
    EMERGENCY_RECALL_CHANNEL_TICKS, EMERGENCY_RECALL_MOVEMENT_BASIS_POINTS, EmergencyRecallState,
    PickupEligibility,
};
pub use bargain::{
    BASIS_POINTS_PER_ONE as BARGAIN_BASIS_POINTS_PER_ONE, BellDebtDefinition,
    CinderHungerDefinition, CoreBargainDefinition, CoreBargainError, CoreBargainKind,
    CoreBargainLoadout, LanternAshDefinition, MAXIMUM_OUTGOING_DAMAGE_BASIS_POINTS,
    MINIMUM_MAXIMUM_HEALTH_BASIS_POINTS, ResolvedCoreBargainModifiers,
    compose_maximum_health_multiplier, compose_outgoing_direct_damage_multiplier,
    resolve_core_bargain_modifiers, resolve_primary_interval_micros,
};
pub use bargain_offer::{
    BARGAIN_CONTENT_ID_MAX_BYTES, BargainOfferError, MAX_ACTIVE_BARGAINS,
    MAX_BARGAIN_OFFER_CANDIDATES, ScoredBargainCandidate, plan_bargain_offer,
    validate_bargain_life_state,
};
pub use boss::{
    BELL_PROCTOR_CROSS_ID, BELL_PROCTOR_FAN_ID, BELL_PROCTOR_ID, BELL_PROCTOR_REWARD_ID,
    BELL_PROCTOR_RING_ID, BellProctorDefinition, BellProctorDefinitionParameters, BellProctorPhase,
    BellProctorSimulation, BellProctorStateKind, BossCastId, BossCueKind, BossDefinitionError,
    BossEvent, BossInput, BossRuntimeError, BossTimelineCue,
};
pub use boss_encounter::{
    BELL_PROCTOR_ENTITY_ID_OFFSET, BellProctorClearedHostiles, BellProctorDamageEvent,
    BellProctorDefeat, BellProctorEncounterError, BellProctorEncounterSimulation,
    BellProctorEncounterSnapshot, BellProctorEncounterStep, BellProctorImmuneStatus,
    BellProctorLaneContact, BellProctorStatusImmunityEvent,
};
pub use clock::{
    FixedStepClock, TICK_RATE_HZ, Tick, duration_ms_to_ticks_ceil, duration_ms_to_ticks_nearest,
};
pub use collision::{
    CollisionError, CollisionTarget, EnemyHurtbox, HurtboxError, ProjectileCollisionWorld,
    ShellSide, SolidColliderId, SweepHit,
};
pub use combat::{
    ActiveGraveMark, AimDirection, AimDirectionError, BELL_DEBT_CHECKPOINT_SCHEMA_VERSION,
    BellDebtCheckpoint, BellDebtPendingRepeatCheckpoint, BellDebtProjectileCheckpoint,
    BellDebtResetReason, CombatAction, CombatError, CombatStep, FocusedTransition,
    FocusedTransitionKind, FriendlyProjectile, FriendlyProjectileSource, GraveMarkInputEvent,
    GraveMarkInputResult, GraveMarkTransition, GraveMarkTransitionKind,
    MAX_BELL_DEBT_CHECKPOINT_BYTES, PlayerCombatState, ProjectileCollision, ProjectileExpired,
    RawDamageIntent, RawDamageIntentSource, ShotEvent, SlipstepInputEvent, SlipstepInputResult,
    SlipstepTransition, SlipstepTransitionKind,
};
pub use consumable::{
    BeltError, BeltSlot, ConsumableAction, ConsumableError, ConsumableEvent, ConsumableStep,
    DamageAppliedEvent, PlayerVitals, RED_TONIC_CONTENT_ID, RED_TONIC_RESTORE_BASIS_POINTS,
    RED_TONIC_RESTORE_TICKS, RED_TONIC_SHARED_COOLDOWN_TICKS, RED_TONIC_STACK_CAP,
    RedTonicDefinition, RedTonicDefinitionError, RedTonicDefinitionParameters, RedTonicSimulation,
    TonicBelt, TonicBeltPolicy, TonicMergeResult, TonicUseRejection,
    UNDERTAKER_KNOT_RESTORE_BASIS_POINTS, UNDERTAKER_KNOT_SHARED_COOLDOWN_TICKS, VitalsError,
};
pub use core_counterplay::{
    CORE_COM006_CLOSE_SPAWN_DISTANCE_MILLI_TILES,
    CORE_COM006_CLOSE_SPAWN_GROUND_WARNING_MILLISECONDS,
    CORE_COM006_MINIMUM_PROJECTILE_ARRIVAL_MILLISECONDS,
    CORE_COM006_NORMAL_SAFE_CORRIDOR_MILLI_TILES, CORE_COM006_PLAYER_HURTBOX_RADIUS_MILLI_TILES,
    CORE_COM006_PLAYER_SPEED_MILLI_TILES_PER_SECOND, CORE_COM006_ROUND_TRIP_LATENCY_MILLISECONDS,
    CORE_COM006_STANDARD_PROJECTILE_CAP, CoreAuthoredMinSpeedPaths, CoreCounterplayDiagnostic,
    CoreCounterplayMotionProof, CoreCounterplayRouteEvidence, CoreCounterplayRouteKind,
    CoreProjectileFairnessProof, solve_core_authored_min_speed_paths,
};
pub use core_enemy::{
    CORE_ENEMY_STATE_SEQUENCE, CoreAttackGroupRule, CoreEnemyDefinition, CoreEnemyDefinitionError,
    CoreEnemyDefinitionParameters, CoreEnemyLocomotionDefinition, CoreEnemyLocomotionParameters,
    CoreEnemyRole, CoreEnemyStateStage, CorePatternDefinition, CorePatternDefinitionParameters,
    CorePatternGeometryDefinition, CorePatternGeometryParameters, CorePatternWarningDefinition,
    CorePatternWarningParameters, CoreRadialGapRelation, CoreTargetSelection, CoreTelegraphLock,
};
pub use core_microrealm::{
    CORE_MICROREALM_EMPTY_RESET_TICKS, CORE_MICROREALM_PACK_WARNING_TICKS,
    CORE_MICROREALM_TRIGGER_DELAY_TICKS, CoreMicrorealmError, CoreMicrorealmEvent,
    CoreMicrorealmInput, CoreMicrorealmPhase, CoreMicrorealmSimulation,
};
pub use damage::{
    DamageBand, DamageBandError, DamageError, DamageEvent, DamageType, DirectHitParameters,
    DirectHitRequest, classify_damage_band, resolve_direct_hit, validate_damage_band,
};
pub use death::{
    LOCAL_DEATH_TRACE_TICKS, LOCAL_RESTART_DEADLINE_TICKS, LocalCombatTraceEntry,
    LocalDamageObservation, LocalDeathCause, LocalDeathCommit, LocalDeathError, LocalDeathId,
    LocalRestartCommit, LocalRunLifecycle, LocalRunPhase, LocalVictoryRestartCommit,
    RunEntityCounts,
};
pub use debug_state::{
    DebugBossState, DebugEnemyState, DebugStateError, LocalDebugStateInput, LocalDebugStateSnapshot,
};
pub use dungeon_room::{
    DungeonAnchorKind, DungeonCorridor, DungeonDoorDefinition, DungeonDoorSide, DungeonRoomAnchor,
    DungeonRoomDefinition, DungeonRoomError, DungeonRoomNavigationEvidence, DungeonRoomVolume,
    DungeonRoomVolumeGeometry, DungeonRoomVolumeKind, FixedDungeonLayoutDefinition,
    PlacedDungeonRoom, RotatedDungeonDoor, RotatedDungeonRoom, WorldDungeonDoor,
};
pub use encounter::{
    BOSS_INTRODUCTION_TICKS, BOSS_REWARD_ID, BellLaboratoryEncounter, EncounterAction,
    EncounterError, EncounterEvent, EncounterInput, EncounterSpawnSpec, EncounterStage,
    EncounterState, EncounterStep, FIRST_PLAYABLE_DEFAULT_SEED, FIRST_WAVE_DELAY_TICKS,
    REWARD_DELAY_TICKS, RecallRejection, RestartReason, SPAWN_TELEGRAPH_TICKS, SpawnInstanceId,
    SpawnLocation, WAVE_1_REWARD_ID, WAVE_2_REWARD_ID, WAVE_3_REWARD_ID,
};
pub use enemy::{
    AimVector, AttackCastId, BELL_REED_ID, BellReedDefinition, BellReedDefinitionParameters,
    BellReedSimulation, CHAIN_SENTRY_ID, ChainSentryDefinition, ChainSentryDefinitionParameters,
    ChainSentrySimulation, Counterplay, DROWNED_PILGRIM_ID, DrownedPilgrimDefinition,
    DrownedPilgrimDefinitionParameters, DrownedPilgrimSimulation, EchoMemoryFamily,
    EnemyDefinitionError, EnemyEvent, EnemyRole, EnemyRuntimeError, EnemyStateKind,
    HostileDisposition, LaneAttackDefinition, NORMAL_ENEMY_REWARD_TABLE_ID, PilgrimTargetInput,
    ProjectileAttackDefinition,
};
pub use enemy_health::{
    EnemyDamageEvent, EnemyDeathEvent, EnemyFrostbindEvent, EnemyHealthActor, EnemyHealthError,
    EnemyHealthSimulation, EnemyHealthSnapshot, EnemyHealthStep, FirstPlayableEnemyKind,
    IgnoredFriendlyIntent, IgnoredIntentReason, NORMAL_REWARD_DROP_DELAY_TICKS,
    NormalRewardDropEvent,
};
pub use enemy_lab::{
    ActiveEnemyLane, ClearedEnemyHostiles, EnemyActorGroup, EnemyLab, EnemyLabActorIds,
    EnemyLabActorPositions, EnemyLabDefinitions, EnemyLabError, EnemyLabPlayer, EnemyLabStep,
    EnemyLabTargetSnapshot, EnemyLaneEvent, EnemyShowcaseReadiness, EnemyTimelineEvent,
};
pub use entity::{EntityId, EntityIdAllocator};
pub use hostile::{
    AppliedHostileDamage, EnemyActor, EnemyActorKind, EnemyActorMovement,
    HOSTILE_PROJECTILE_GRACE_TICKS, HostileCollisionTarget, HostileDamagePolicy, HostileError,
    HostileEvent, HostileProjectile, HostileProjectileSimulation, HostileProjectileSourceKind,
    HostileStep, HostileTargetState, LaneGeometry, PLAYER_HURTBOX_RADIUS_TILES,
    apply_hostile_contact_transaction, apply_hostile_contact_transaction_with_policy,
    resolve_lane_contact, resolve_lane_contact_with_policy,
};
pub use inventory::{
    AUTOMATIC_PICKUP_RADIUS_TILES, EQUIPMENT_SLOT_COUNT, EquipmentItem, EquipmentSlot,
    FIELD_PICKUP_LIFETIME_TICKS, FieldPickup, FieldPickupAccess, FieldPickupId,
    INTERACT_PICKUP_RADIUS_TILES, InventoryError, InventoryStack, ItemContentId, ItemInstanceId,
    OwnedItemLocation, PROTOTYPE_BACKPACK_CAPACITY, PickupOutcome, PlacementChoice,
    PrototypeInventory, RecallCleanup, RestartCleanup, RewardChoice, RewardOutcome,
};
pub use item_lifecycle::{
    ConsumablePlacementPlan, DURABLE_CONSUMABLE_STACK_CAP, EquipmentPlacementPlan, ITEM_UID_BYTES,
    ITEM_UID_CONTEXT, ItemLifecycleError, ItemUid, RUN_BACKPACK_CAPACITY, RunBackpackSlot,
    STARTER_UID_CONTEXT, StackPlacement, derive_reward_item_uid, derive_starter_item_uid,
    plan_consumable_reward_placement, plan_equipment_reward_placement,
};
pub use movement::{
    ForcedMovementStep, GRAVE_ARBALIST_SPEED_TILES_PER_SECOND, MOVEMENT_RESPONSE_TICKS,
    MovementAction, MovementError, MovementStep, PLAYER_COLLISION_RADIUS_MILLI_TILES,
    PLAYER_COLLISION_RADIUS_TILES, PlayerMovementConfig, PlayerMovementState, SimulationVector,
    tile_point_to_simulation,
};
pub use normal_wave::{
    FIRST_PLAYABLE_SPAWN_TELEGRAPH_TICKS, HOSTILE_PROJECTILE_ID_OFFSET,
    NORMAL_WAVE_ENEMY_ID_OFFSET, NORMAL_WAVE_MAX_SPAWN_ORDINAL, NormalWaveClearedHostiles,
    NormalWaveDefeat, NormalWaveDefinitions, NormalWaveDrop, NormalWaveEnemyKind,
    NormalWaveEntityIdError, NormalWaveError, NormalWaveHandoff, NormalWaveInstanceSnapshot,
    NormalWaveLaneEvent, NormalWavePhase, NormalWaveSimulation, NormalWaveSpawn, NormalWaveStep,
    NormalWaveTimelineEvent, RUN_ENTITY_ID_STRIDE, normal_wave_entity_id,
    normal_wave_projectile_allocator,
};
pub use oath::{
    GraveArbalistOath, LONG_VIGIL_FOCUSED_ACTIVATION_TICKS,
    LONG_VIGIL_GRAVE_MARK_RANGE_BONUS_MILLI_TILES, LONG_VIGIL_ID,
    LONG_VIGIL_MARKED_PRIMARY_BONUS_BASIS_POINTS, LONG_VIGIL_MAX_HEALTH_MULTIPLIER_BASIS_POINTS,
    NAILKEEPER_ARM_TICKS, NAILKEEPER_DAMAGE_BASIS_POINTS, NAILKEEPER_FROSTBIND_TICKS,
    NAILKEEPER_ID, NAILKEEPER_LIFETIME_TICKS, NAILKEEPER_MAXIMUM_ACTIVE_TRAPS,
    NAILKEEPER_PRIMARY_INTERVAL_MULTIPLIER_BASIS_POINTS, NAILKEEPER_TRAP_RADIUS_MILLI_TILES,
    NAILKEEPER_TRAP_RADIUS_TILES, NailTrap, NailTrapEnemy, NailTrapField, NailTrapRemoval,
    NailTrapRemovalReason, NailTrapStep, NailTrapTrigger, OathMechanicError,
    ResolvedArbalistOathStats, resolve_arbalist_oath_stats, resolve_oath_maximum_health,
};
pub use pattern::{
    CombatColorFamily, FirstPlayableMinSpeedPaths, FixedTimelineEvent, MinimumSpeedRouteEvidence,
    MinimumSpeedRouteKind, OriginCue, PatternContext, PatternDefinition, PatternDiagnostic,
    PatternFairnessFixture, PatternKind, PatternStatus, ShapeCue, TimelineAction, ValidatedPattern,
    frostbind_compatibility_speed_milli_tiles_per_second, minimum_warnings, projectile_arrival_ms,
    required_player_center_boundary_clearance_milli_tiles, safe_player_center_span_milli_tiles,
    solve_first_playable_min_speed_paths, validate_pattern_combination,
};
pub use performance::{
    BOSS_RELIABILITY_RUN_COUNT, BOSS_REPLAY_TICKS, BossReliabilityReport, EffectMode,
    FrameSampleKind, MONOTONIC_GROWTH_FLOOR_BYTES, MemoryAssessment, MemorySample,
    PerformanceAcceptance, PerformanceEvidenceInput, PerformanceEvidenceReport,
    PerformanceReportError, StressFixture, StressFixtureConfig, StressFixtureSnapshot,
    TARGET_ENEMY_COUNT, TARGET_HOSTILE_PROJECTILE_COUNT, TargetHardware,
    run_bell_proctor_reliability_fixture,
};
pub use production_item::{
    ArmorBaseRequest, CrossbowPowerRequest, EquipmentRarity, ProductionItemMathError,
    ResolvedArmorBase, resolve_armor_base, resolve_crossbow_weapon_power,
};
pub use progression::{
    CORE_LEVEL_COUNT, CoreProgressionError, CoreProgressionGrant, CoreProgressionState,
    EncounterXpEvidence, GraveArbalistLevelStats, GraveArbalistProgressionDefinition, LevelCurve,
    NORMAL_XP_CONTRIBUTION_WINDOW_TICKS, NORMAL_XP_RADIUS_MILLI_TILES, NormalXpEvidence,
    RewardLifeState, RewardRecallState, RewardTrustState, SOC_INACTIVITY_LIMIT_TICKS,
    SOC_SHORT_ENCOUNTER_TICKS, XpEligibility, apply_core_xp, evaluate_encounter_xp_eligibility,
    evaluate_normal_xp_eligibility, first_clear_bonus, grave_arbalist_level_stats,
    rebuild_current_health,
};
pub use readability::{
    CombatEffectLayer, GrayscaleSignature, HostileReadabilityManifest, HostileReadabilityProfile,
    OutlineTreatment, ReadabilityDiagnostic, TelegraphExposureError, TelegraphExposureEvent,
    TelegraphExposureState, TelegraphExposureTracker, TelegraphUse, WarningAudioPriority,
    canonical_priority_stack_is_valid, compile_hostile_readability_manifest,
};
pub use rng::{DeterministicRng, RngError, derive_stream_seed};
pub use scene_interaction::{
    SceneInteractionEvent, SceneInteractionRejection, SceneInteractionSession,
    SceneInteractionSessionError,
};
pub use shared_authority::{
    SHARED_ARENA_MAX_PLAYERS, SHARED_FRIENDLY_PROJECTILE_ID_BASE,
    SHARED_FRIENDLY_PROJECTILE_ID_STRIDE, SharedArenaPlayer, SharedAuthoritativeArena,
    SharedAuthorityError, SharedAuthorityStep,
};
pub use telemetry::{
    BossPhaseTelemetry, CohortEligibility, DamageTelemetry, DeathCauseTelemetry, DeathTelemetry,
    GenreFamiliarity, ItemLifecycleAction, ItemLifecycleTelemetry, KillerResponseTelemetry,
    LOCAL_ACCOUNT_SENTINEL, LOCAL_ENVIRONMENT, LOCAL_REGION, LocalTelemetryContext,
    LocalTelemetryError, LocalTelemetryLog, MetricEligibility, ObservationMoment,
    ObservationTelemetry, OpenQuestion, OpenSurveyAnswer, PrivacySafeSurveySummary, Rating,
    RestartReasonTelemetry, RestartTelemetry, SurveyTelemetry, TELEMETRY_SCHEMA_VERSION,
    TelemetryEnvelope, TelemetryEvent, TelemetryEventKind, TelemetryRecord,
};
pub use trace::{
    FoundationEntity, FoundationSimulation, InputFrame, TickHash, TraceError, TraceFixture,
    TraceReport, run_trace,
};
pub use weapon::{WeaponDefinition, WeaponDefinitionError, WeaponDefinitionParameters};
pub use world_scene::{
    InteractionDefinition, SceneAccessContext, SceneCreationKind, SceneDisplacement,
    SceneInteractionAccess, SceneInteractionProjection, SceneObjectCondition, SceneObjectGeometry,
    WorldRoad, WorldSceneDefinition, WorldSceneError, WorldSceneKind, WorldSceneObject,
    WorldScenePlayer,
};

/// Authoritative simulation frequency required by `TECH-070` and `GB-M00-05`.
pub const TICKS_PER_SECOND: u32 = TICK_RATE_HZ;

/// Returns the crate's immutable diagnostic version.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
