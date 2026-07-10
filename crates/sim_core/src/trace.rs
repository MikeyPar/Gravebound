use std::{collections::BTreeMap, num::NonZeroU64};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DeterministicRng, EntityId, EntityIdAllocator, Tick};

/// Current deterministic trace fixture schema.
pub const TRACE_SCHEMA_VERSION: u32 = 1;

/// Minimal authoritative entity used to prove foundation determinism before combat exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoundationEntity {
    pub entity_id: EntityId,
    pub position_x_milli: i64,
    pub position_y_milli: i64,
    pub health: u32,
}

/// One fixture input applied at the beginning of an authoritative tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputFrame {
    pub tick: Tick,
    pub entity_id: EntityId,
    pub delta_x_milli: i32,
    pub delta_y_milli: i32,
    #[serde(default)]
    pub damage: u32,
}

/// Serializable deterministic trace definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceFixture {
    pub schema_version: u32,
    pub feature_id: String,
    pub content_version: String,
    pub root_seed: u64,
    pub total_ticks: u64,
    pub selected_ticks: Vec<Tick>,
    pub entities: Vec<FoundationEntity>,
    pub inputs: Vec<InputFrame>,
}

/// State hash captured after a selected simulation tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickHash {
    pub tick: Tick,
    pub state_hash_blake3: String,
}

/// Stable output checked into source control as a golden deterministic result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceReport {
    pub schema_version: u32,
    pub feature_id: String,
    pub content_version: String,
    pub root_seed: u64,
    pub tick_hashes: Vec<TickHash>,
}

/// Deterministic foundation trace failure.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TraceError {
    #[error("unsupported trace schema {actual}; expected {expected}")]
    UnsupportedSchema { actual: u32, expected: u32 },
    #[error("trace must select at least one hash tick")]
    NoSelectedTicks,
    #[error("selected tick {0} is outside 1..=total_ticks")]
    SelectedTickOutOfRange(Tick),
    #[error("input tick {0} is outside 1..=total_ticks")]
    InputTickOutOfRange(Tick),
    #[error("duplicate foundation entity ID {0}")]
    DuplicateEntity(EntityId),
    #[error("duplicate input for entity {entity_id} at tick {tick:?}")]
    DuplicateInput { entity_id: EntityId, tick: Tick },
    #[error("input references unknown entity ID {0}")]
    UnknownEntity(EntityId),
    #[error("simulation tick or entity ID range was exhausted")]
    ArithmeticExhausted,
}

/// Renderer-independent state container used by the trace runner.
#[derive(Debug, Clone)]
pub struct FoundationSimulation {
    tick: Tick,
    entities: BTreeMap<EntityId, FoundationEntity>,
    entity_ids: EntityIdAllocator,
    fixture_rng: DeterministicRng,
    latest_random_probe: u64,
}

impl FoundationSimulation {
    /// Creates canonical state, rejecting duplicate IDs and exhausted allocator ranges.
    pub fn new(
        content_version: &str,
        root_seed: u64,
        entities: Vec<FoundationEntity>,
    ) -> Result<Self, TraceError> {
        let mut by_id = BTreeMap::new();
        let mut maximum_id = 0_u64;
        for entity in entities {
            let entity_id = entity.entity_id;
            maximum_id = maximum_id.max(entity_id.get());
            if by_id.insert(entity_id, entity).is_some() {
                return Err(TraceError::DuplicateEntity(entity_id));
            }
        }
        let next_id = maximum_id
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(TraceError::ArithmeticExhausted)?;

        Ok(Self {
            tick: Tick(0),
            entities: by_id,
            entity_ids: EntityIdAllocator::starting_at(next_id),
            fixture_rng: DeterministicRng::new(content_version, root_seed, "fixture"),
            latest_random_probe: 0,
        })
    }

    /// Advances exactly one tick after applying inputs in stable entity-ID order.
    pub fn step(&mut self, inputs: &[InputFrame]) -> Result<Tick, TraceError> {
        let next_tick = self
            .tick
            .checked_next()
            .ok_or(TraceError::ArithmeticExhausted)?;
        let mut ordered = inputs.to_vec();
        ordered.sort_by_key(|input| input.entity_id);
        let mut previous_entity_id = None;

        for input in ordered {
            if input.tick != next_tick {
                return Err(TraceError::InputTickOutOfRange(input.tick));
            }
            if previous_entity_id == Some(input.entity_id) {
                return Err(TraceError::DuplicateInput {
                    entity_id: input.entity_id,
                    tick: input.tick,
                });
            }
            previous_entity_id = Some(input.entity_id);
            let entity = self
                .entities
                .get_mut(&input.entity_id)
                .ok_or(TraceError::UnknownEntity(input.entity_id))?;
            entity.position_x_milli = entity
                .position_x_milli
                .checked_add(i64::from(input.delta_x_milli))
                .ok_or(TraceError::ArithmeticExhausted)?;
            entity.position_y_milli = entity
                .position_y_milli
                .checked_add(i64::from(input.delta_y_milli))
                .ok_or(TraceError::ArithmeticExhausted)?;
            entity.health = entity.health.saturating_sub(input.damage);
        }

        self.latest_random_probe = self.fixture_rng.next_u64();
        self.tick = next_tick;
        Ok(self.tick)
    }

    /// Hashes explicitly encoded authoritative fields in canonical order.
    #[must_use]
    pub fn state_hash(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"gravebound-foundation-state-v1\0");
        hasher.update(&self.tick.0.to_le_bytes());
        hasher.update(&self.entity_ids.peek().get().to_le_bytes());
        hasher.update(&(self.entities.len() as u64).to_le_bytes());
        for entity in self.entities.values() {
            hasher.update(&entity.entity_id.get().to_le_bytes());
            hasher.update(&entity.position_x_milli.to_le_bytes());
            hasher.update(&entity.position_y_milli.to_le_bytes());
            hasher.update(&entity.health.to_le_bytes());
        }
        hasher.update(&self.latest_random_probe.to_le_bytes());
        hasher.finalize().to_hex().to_string()
    }
}

/// Runs a fixture and returns the selected canonical hashes.
pub fn run_trace(fixture: &TraceFixture) -> Result<TraceReport, TraceError> {
    validate_fixture(fixture)?;
    let mut simulation = FoundationSimulation::new(
        &fixture.content_version,
        fixture.root_seed,
        fixture.entities.clone(),
    )?;
    let selected: std::collections::BTreeSet<_> = fixture.selected_ticks.iter().copied().collect();
    let mut tick_hashes = Vec::with_capacity(selected.len());

    for raw_tick in 1..=fixture.total_ticks {
        let tick = Tick(raw_tick);
        let inputs: Vec<_> = fixture
            .inputs
            .iter()
            .filter(|input| input.tick == tick)
            .cloned()
            .collect();
        simulation.step(&inputs)?;
        if selected.contains(&tick) {
            tick_hashes.push(TickHash {
                tick,
                state_hash_blake3: simulation.state_hash(),
            });
        }
    }

    Ok(TraceReport {
        schema_version: TRACE_SCHEMA_VERSION,
        feature_id: fixture.feature_id.clone(),
        content_version: fixture.content_version.clone(),
        root_seed: fixture.root_seed,
        tick_hashes,
    })
}

fn validate_fixture(fixture: &TraceFixture) -> Result<(), TraceError> {
    if fixture.schema_version != TRACE_SCHEMA_VERSION {
        return Err(TraceError::UnsupportedSchema {
            actual: fixture.schema_version,
            expected: TRACE_SCHEMA_VERSION,
        });
    }
    if fixture.selected_ticks.is_empty() {
        return Err(TraceError::NoSelectedTicks);
    }
    for tick in &fixture.selected_ticks {
        if tick.0 == 0 || tick.0 > fixture.total_ticks {
            return Err(TraceError::SelectedTickOutOfRange(*tick));
        }
    }
    for input in &fixture.inputs {
        if input.tick.0 == 0 || input.tick.0 > fixture.total_ticks {
            return Err(TraceError::InputTickOutOfRange(input.tick));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity_id(value: u64) -> EntityId {
        EntityId::new(value).expect("nonzero test ID")
    }

    #[test]
    fn a_fixture_replays_to_the_same_hashes() {
        let fixture = TraceFixture {
            schema_version: TRACE_SCHEMA_VERSION,
            feature_id: "GB-M00-08".to_owned(),
            content_version: "fp.1.0.0".to_owned(),
            root_seed: 42,
            total_ticks: 3,
            selected_ticks: vec![Tick(1), Tick(3)],
            entities: vec![FoundationEntity {
                entity_id: entity_id(7),
                position_x_milli: 0,
                position_y_milli: 0,
                health: 100,
            }],
            inputs: vec![InputFrame {
                tick: Tick(2),
                entity_id: entity_id(7),
                delta_x_milli: 125,
                delta_y_milli: -50,
                damage: 9,
            }],
        };

        let mut reordered = fixture.clone();
        reordered.entities.reverse();
        assert_eq!(run_trace(&fixture), run_trace(&reordered));
    }

    #[test]
    fn duplicate_input_is_rejected() {
        let id = entity_id(1);
        let mut simulation = FoundationSimulation::new(
            "fp.1.0.0",
            1,
            vec![FoundationEntity {
                entity_id: id,
                position_x_milli: 0,
                position_y_milli: 0,
                health: 1,
            }],
        )
        .expect("simulation");
        let input = InputFrame {
            tick: Tick(1),
            entity_id: id,
            delta_x_milli: 0,
            delta_y_milli: 0,
            damage: 0,
        };

        assert_eq!(
            simulation.step(&[input.clone(), input]),
            Err(TraceError::DuplicateInput {
                entity_id: id,
                tick: Tick(1)
            })
        );
    }
}
