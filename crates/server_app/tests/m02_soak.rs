use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bot_client::{BotBehavior, BotTerminalOutcome, JourneyBot};
use protocol::{ActionKind, SessionControlFrame, WireMessage, decode_frame, encode_frame};
use serde::Serialize;
use server_app::{
    InstanceScheduler, M02_SOAK_BOT_COUNT, M02_SOAK_DURATION_TICKS, SessionOwnerId, TransportId,
};
use sysinfo::{Pid, ProcessesToUpdate, System, get_current_pid};

const RECONNECT_INTERVAL_TICKS: u64 = 15 * 60 * 30;
const MEMORY_SAMPLE_INTERVAL_TICKS: u64 = 30 * 60 * 30;

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

fn protocol_round_trip(message: &WireMessage) -> WireMessage {
    decode_frame(&encode_frame(message).expect("canonical soak encode"))
        .expect("canonical soak decode")
}

#[derive(Debug)]
struct SoakSlot {
    bot: JourneyBot,
    behavior: BotBehavior,
    owner: SessionOwnerId,
    transport: TransportId,
    generation: u64,
    recall_started: bool,
}

impl SoakSlot {
    fn behavior(index: usize, generation: u64) -> BotBehavior {
        if (u64::try_from(index).unwrap() + generation).is_multiple_of(4) {
            BotBehavior::AwaitAuthoritativeDeath
        } else {
            BotBehavior::FightAndCollect
        }
    }

    fn new(index: usize, generation: u64, identity: u64) -> Self {
        let behavior = Self::behavior(index, generation);
        Self {
            bot: JourneyBot::with_behavior(behavior),
            behavior,
            owner: SessionOwnerId::new(identity).unwrap(),
            transport: TransportId::new(identity).unwrap(),
            generation,
            recall_started: false,
        }
    }
}

struct ProcessMemorySampler {
    system: System,
    pid: Pid,
}

impl ProcessMemorySampler {
    fn new() -> Self {
        Self {
            system: System::new(),
            pid: get_current_pid().expect("soak process identity"),
        }
    }

    fn resident_bytes(&mut self) -> u64 {
        self.system
            .refresh_processes(ProcessesToUpdate::Some(&[self.pid]), true);
        self.system
            .process(self.pid)
            .expect("soak process remains available")
            .memory()
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
struct SoakMemorySample {
    simulated_tick: u64,
    resident_bytes: u64,
}

#[derive(Debug, Serialize)]
struct SoakEvidence {
    schema_version: u16,
    feature_id: &'static str,
    content_version: &'static str,
    bot_count: usize,
    scheduler_frames: u64,
    simulated_seconds: u64,
    session_generations: u64,
    inputs: u64,
    snapshot_chunks: u64,
    accepted_mutations: u64,
    recalls: u64,
    deaths: u64,
    reconnects: u64,
    maximum_instances: usize,
    maximum_owners: usize,
    allocated_instances: u64,
    closed_instances: u64,
    retired_sessions: u64,
    invalid_states: u64,
    simulation_stalls: u64,
    state_divergences: u64,
    mean_tick_micros: u64,
    p95_tick_micros: u64,
    p99_tick_micros: u64,
    maximum_tick_micros: u64,
    mean_headroom_basis_points: u16,
    resident_memory_samples: Vec<SoakMemorySample>,
    post_warmup_growth_bytes: u64,
    monotonic_post_warmup_leak: bool,
    zero_residue_after_shutdown: bool,
}

#[derive(Debug, Default)]
struct SoakCounters {
    next_identity: u64,
    generations: u64,
    inputs: u64,
    snapshot_chunks: u64,
    accepted_mutations: u64,
    recalls: u64,
    deaths: u64,
    reconnects: u64,
    maximum_instances: usize,
    maximum_owners: usize,
    state_divergences: u64,
}

impl SoakCounters {
    fn allocate_identity(&mut self) -> u64 {
        self.next_identity = self.next_identity.checked_add(1).expect("identity bound");
        self.generations = self.generations.checked_add(1).expect("generation bound");
        self.next_identity
    }
}

fn deliver_reliable(bot: &mut JourneyBot, response: &WireMessage) {
    let WireMessage::ReliableEvent(event) = protocol_round_trip(response) else {
        panic!("server reliable result kind");
    };
    bot.apply_reliable_event(&event)
        .expect("bot accepts authoritative reliable result");
}

fn admit_slot(
    scheduler: &mut InstanceScheduler,
    slot: &mut SoakSlot,
    frame: SessionControlFrame,
    monotonic_micros: u64,
) {
    let WireMessage::SessionControlFrame(frame) =
        protocol_round_trip(&WireMessage::SessionControlFrame(frame))
    else {
        panic!("control round trip kind");
    };
    let response = scheduler
        .admit_or_route_control(
            slot.owner,
            slot.transport,
            &frame,
            &content_root(),
            monotonic_micros,
        )
        .expect("instance control");
    deliver_reliable(
        &mut slot.bot,
        &WireMessage::ReliableEvent(response.lifecycle.event),
    );
}

#[allow(clippy::too_many_lines)] // One continuous population run preserves audit ordering.
fn run_soak(bot_count: usize, duration_ticks: u64) -> SoakEvidence {
    let mut scheduler = InstanceScheduler::default();
    let mut counters = SoakCounters::default();
    let mut slots = (0..bot_count)
        .map(|index| {
            let identity = counters.allocate_identity();
            let mut slot = SoakSlot::new(index, 1, identity);
            let join = slot.bot.next_join(0).expect("initial bot Join");
            admit_slot(&mut scheduler, &mut slot, join, 0);
            slot
        })
        .collect::<Vec<_>>();
    let mut memory = ProcessMemorySampler::new();
    let mut memory_samples = vec![SoakMemorySample {
        simulated_tick: 0,
        resident_bytes: memory.resident_bytes(),
    }];

    for scheduler_tick in 1..=duration_ticks {
        if scheduler_tick.is_multiple_of(RECONNECT_INTERVAL_TICKS) {
            for slot in &mut slots {
                counters.next_identity = counters
                    .next_identity
                    .checked_add(1)
                    .expect("transport identity bound");
                slot.transport = TransportId::new(counters.next_identity).unwrap();
                let reconnect = slot
                    .bot
                    .next_reconnect(scheduler_tick.saturating_mul(33_333))
                    .expect("active reconnect");
                admit_slot(
                    &mut scheduler,
                    slot,
                    reconnect,
                    scheduler_tick.saturating_mul(33_333),
                );
                counters.reconnects = counters.reconnects.checked_add(1).unwrap();
            }
        }

        for slot in &mut slots {
            let input = slot.bot.next_input().expect("active soak bot");
            let WireMessage::InputFrame(input) =
                protocol_round_trip(&WireMessage::InputFrame(input))
            else {
                panic!("input round trip kind");
            };
            assert_eq!(
                scheduler
                    .submit_input(slot.owner, slot.transport, &input)
                    .expect("scheduled bot input"),
                server_app::InputDisposition::Accepted
            );
            counters.inputs = counters.inputs.checked_add(1).unwrap();
        }

        let frame = scheduler.tick().expect("scheduled authority frame");
        assert_eq!(frame.scheduler_tick, scheduler_tick);
        assert_eq!(frame.session_steps, bot_count);
        let mut cohort_snapshots = BTreeMap::<(u8, u64, u16), Vec<u8>>::new();
        for batch in frame.snapshot_batches {
            let slot = slots
                .iter_mut()
                .find(|slot| slot.owner == batch.owner)
                .expect("snapshot owner slot");
            for snapshot in batch.snapshots {
                let behavior_code = match slot.behavior {
                    BotBehavior::FightAndCollect => 0,
                    BotBehavior::AwaitAuthoritativeDeath => 1,
                };
                let key = (behavior_code, snapshot.server_tick, snapshot.chunk_index);
                let encoded = encode_frame(&WireMessage::SnapshotChunk(snapshot.clone()))
                    .expect("cohort snapshot encode");
                if let Some(expected) = cohort_snapshots.get(&key) {
                    if expected != &encoded {
                        counters.state_divergences =
                            counters.state_divergences.checked_add(1).unwrap();
                    }
                } else {
                    cohort_snapshots.insert(key, encoded);
                }
                let WireMessage::SnapshotChunk(snapshot) =
                    protocol_round_trip(&WireMessage::SnapshotChunk(snapshot))
                else {
                    panic!("snapshot round trip kind");
                };
                slot.bot
                    .ingest_snapshot(snapshot)
                    .expect("bot snapshot assembly");
                counters.snapshot_chunks = counters.snapshot_chunks.checked_add(1).unwrap();
            }
        }

        for slot in &mut slots {
            if let Some(request) = slot.bot.next_pickup_request().expect("soak pickup policy") {
                let WireMessage::MutationRequest(request) =
                    protocol_round_trip(&WireMessage::MutationRequest(request))
                else {
                    panic!("mutation round trip kind");
                };
                let response = scheduler
                    .handle_gameplay_reliable(
                        slot.owner,
                        slot.transport,
                        WireMessage::MutationRequest(request),
                    )
                    .expect("scheduled mutation");
                deliver_reliable(&mut slot.bot, &response);
                counters.accepted_mutations = counters.accepted_mutations.checked_add(1).unwrap();
            }
            if !slot.recall_started
                && slot.bot.evidence().mutations_accepted > 0
                && slot.bot.terminal_outcome() == BotTerminalOutcome::Active
            {
                let action = slot
                    .bot
                    .next_action(ActionKind::RecallStart)
                    .expect("soak Recall start");
                let WireMessage::ActionFrame(action) =
                    protocol_round_trip(&WireMessage::ActionFrame(action))
                else {
                    panic!("action round trip kind");
                };
                let response = scheduler
                    .handle_gameplay_reliable(
                        slot.owner,
                        slot.transport,
                        WireMessage::ActionFrame(action),
                    )
                    .expect("scheduled Recall start");
                deliver_reliable(&mut slot.bot, &response);
                slot.recall_started = true;
            }
        }

        let terminal_owners = slots
            .iter()
            .filter(|slot| slot.bot.terminal_outcome() != BotTerminalOutcome::Active)
            .map(|slot| slot.owner)
            .collect::<BTreeSet<_>>();
        if !terminal_owners.is_empty() {
            for slot in slots
                .iter()
                .filter(|slot| terminal_owners.contains(&slot.owner))
            {
                match slot.bot.terminal_outcome() {
                    BotTerminalOutcome::Recalled => {
                        counters.recalls = counters.recalls.checked_add(1).unwrap();
                    }
                    BotTerminalOutcome::Dead => {
                        counters.deaths = counters.deaths.checked_add(1).unwrap();
                    }
                    BotTerminalOutcome::Active => unreachable!(),
                }
            }
            let retired = scheduler.retire_resolved().expect("resolved retirement");
            assert_eq!(
                retired.iter().copied().collect::<BTreeSet<_>>(),
                terminal_owners
            );
            for (index, slot) in slots.iter_mut().enumerate() {
                if terminal_owners.contains(&slot.owner) {
                    let generation = slot.generation.checked_add(1).unwrap();
                    let identity = counters.allocate_identity();
                    *slot = SoakSlot::new(index, generation, identity);
                    let join = slot
                        .bot
                        .next_join(scheduler_tick.saturating_mul(33_333))
                        .expect("successor Join");
                    admit_slot(
                        &mut scheduler,
                        slot,
                        join,
                        scheduler_tick.saturating_mul(33_333),
                    );
                }
            }
        }

        counters.maximum_instances = counters.maximum_instances.max(scheduler.instance_count());
        counters.maximum_owners = counters.maximum_owners.max(scheduler.owner_count());
        assert_eq!(scheduler.owner_count(), bot_count);
        if scheduler_tick.is_multiple_of(MEMORY_SAMPLE_INTERVAL_TICKS) {
            memory_samples.push(SoakMemorySample {
                simulated_tick: scheduler_tick,
                resident_bytes: memory.resident_bytes(),
            });
        }
    }

    let post_warmup = memory_samples.iter().skip(1).collect::<Vec<_>>();
    let post_warmup_growth_bytes = post_warmup
        .last()
        .map_or(0, |sample| sample.resident_bytes)
        .saturating_sub(
            post_warmup
                .first()
                .map_or(0, |sample| sample.resident_bytes),
        );
    let monotonic_post_warmup_leak = post_warmup_growth_bytes
        >= sim_core::MONOTONIC_GROWTH_FLOOR_BYTES
        && post_warmup.len() >= 2
        && post_warmup
            .windows(2)
            .all(|pair| pair[0].resident_bytes < pair[1].resident_bytes);
    let timing = scheduler
        .diagnostics()
        .timing_report()
        .expect("timing report")
        .expect("timing samples");
    assert!(timing.passes_m02_limits);
    assert_eq!(scheduler.diagnostics().invalid_states, 0);
    assert_eq!(scheduler.diagnostics().simulation_stalls, 0);
    assert_eq!(counters.state_divergences, 0);
    assert!(
        !monotonic_post_warmup_leak,
        "resident memory grew monotonically after warmup: {memory_samples:?}"
    );
    assert!(counters.accepted_mutations > 0);
    assert!(counters.recalls > 0);
    if duration_ticks >= RECONNECT_INTERVAL_TICKS {
        assert!(counters.deaths > 0);
        assert!(counters.reconnects > 0);
    }
    assert_eq!(counters.maximum_owners, bot_count);
    assert!(counters.maximum_instances <= 1);

    let allocated_instances = scheduler.diagnostics().allocated_instances;
    let retired_sessions = scheduler.diagnostics().retired_sessions;
    let invalid_states = scheduler.diagnostics().invalid_states;
    let simulation_stalls = scheduler.diagnostics().simulation_stalls;
    scheduler.begin_shutdown().expect("soak shutdown begins");
    scheduler.finish_shutdown().expect("soak shutdown finishes");
    let zero_residue_after_shutdown =
        scheduler.instance_count() == 0 && scheduler.owner_count() == 0;
    assert!(zero_residue_after_shutdown);

    SoakEvidence {
        schema_version: 1,
        feature_id: "GB-M02-08",
        content_version: "fp.1.0.0",
        bot_count,
        scheduler_frames: duration_ticks,
        simulated_seconds: duration_ticks / 30,
        session_generations: counters.generations,
        inputs: counters.inputs,
        snapshot_chunks: counters.snapshot_chunks,
        accepted_mutations: counters.accepted_mutations,
        recalls: counters.recalls,
        deaths: counters.deaths,
        reconnects: counters.reconnects,
        maximum_instances: counters.maximum_instances,
        maximum_owners: counters.maximum_owners,
        allocated_instances,
        closed_instances: scheduler.diagnostics().closed_instances,
        retired_sessions,
        invalid_states,
        simulation_stalls,
        state_divergences: counters.state_divergences,
        mean_tick_micros: timing.mean_micros,
        p95_tick_micros: timing.p95_micros,
        p99_tick_micros: timing.p99_micros,
        maximum_tick_micros: timing.maximum_micros,
        mean_headroom_basis_points: timing.mean_headroom_basis_points,
        resident_memory_samples: memory_samples,
        post_warmup_growth_bytes,
        monotonic_post_warmup_leak,
        zero_residue_after_shutdown,
    }
}

#[test]
fn m02_instance_soak_smoke_uses_non_idle_protocol_bots() {
    let evidence = run_soak(4, 600);
    assert_eq!(evidence.bot_count, 4);
    assert_eq!(evidence.scheduler_frames, 600);
    assert_eq!(evidence.inputs, 2_400);
    assert!(evidence.snapshot_chunks > 0);
    assert!(evidence.session_generations >= 4);
    assert!(evidence.zero_residue_after_shutdown);
}

#[test]
#[ignore = "explicit release-profile sixteen-bot/two-simulated-hour M02 gate"]
fn m02_sixteen_bot_two_hour_soak() {
    let evidence = run_soak(M02_SOAK_BOT_COUNT, M02_SOAK_DURATION_TICKS);
    println!(
        "M02_SOAK_EVIDENCE={}",
        serde_json::to_string(&evidence).expect("serialize M02 soak evidence")
    );
}
