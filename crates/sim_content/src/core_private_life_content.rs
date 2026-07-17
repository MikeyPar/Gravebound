//! Immutable content bundle for the ordinary M03 private-life actor.
//!
//! `Gravebound_Production_GDD_v1_Canonical.md` defines the Hall/danger authority boundary,
//! `Gravebound_Content_Production_Spec_v1.md` defines the exact Core micro-realm, fixed Bell
//! layout, and Sir Caldus content, and `Gravebound_Development_Roadmap_v1.md` requires those
//! pieces to compose into one `GB-M03-03` route. This module only compiles and identifies that
//! content. It cannot promote content or enable player admission.

use std::path::Path;

use anyhow::{Context, Result, bail};
use sim_core::{FixedDungeonLayoutDefinition, WorldSceneDefinition};

use crate::{
    CoreCaldusHashes, CoreDevelopmentCaldus, CoreDevelopmentEncounterRooms,
    CoreDevelopmentWorldFlow, CoreEncounterRoomHashes, CoreWorldFlowHashes,
    load_core_development_caldus, load_core_development_encounter_rooms,
    load_core_development_world_flow,
};

const PRIVATE_LIFE_RECORDS_DOMAIN: &[u8] = b"gravebound/core-private-life/content/records/v1\0";
const PRIVATE_LIFE_ASSETS_DOMAIN: &[u8] = b"gravebound/core-private-life/content/assets/v1\0";
const PRIVATE_LIFE_LOCALIZATION_DOMAIN: &[u8] =
    b"gravebound/core-private-life/content/localization/v1\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePrivateLifeContentRevision {
    pub records_blake3: String,
    pub assets_blake3: String,
    pub localization_blake3: String,
}

/// Fully validated immutable inputs for the capacity-one M03 route. Instance-local identities,
/// warning ticks, participant locks, and runtime simulations are intentionally constructed by the
/// actor rather than stored in this process-global bundle.
#[derive(Debug, Clone)]
pub struct CorePrivateLifeContent {
    world_flow: CoreDevelopmentWorldFlow,
    encounter_rooms: CoreDevelopmentEncounterRooms,
    caldus: CoreDevelopmentCaldus,
    hall_scene: WorldSceneDefinition,
    microrealm_scene: WorldSceneDefinition,
    fixed_layout: FixedDungeonLayoutDefinition,
    revision: CorePrivateLifeContentRevision,
}

impl CorePrivateLifeContent {
    #[must_use]
    pub const fn world_flow(&self) -> &CoreDevelopmentWorldFlow {
        &self.world_flow
    }

    #[must_use]
    pub const fn encounter_rooms(&self) -> &CoreDevelopmentEncounterRooms {
        &self.encounter_rooms
    }

    #[must_use]
    pub const fn caldus(&self) -> &CoreDevelopmentCaldus {
        &self.caldus
    }

    #[must_use]
    pub const fn hall_scene(&self) -> &WorldSceneDefinition {
        &self.hall_scene
    }

    #[must_use]
    pub const fn microrealm_scene(&self) -> &WorldSceneDefinition {
        &self.microrealm_scene
    }

    #[must_use]
    pub const fn fixed_layout(&self) -> &FixedDungeonLayoutDefinition {
        &self.fixed_layout
    }

    #[must_use]
    pub const fn revision(&self) -> &CorePrivateLifeContentRevision {
        &self.revision
    }
}

/// Loads every route component before a normal server endpoint can bind. Any source, hash,
/// compiler, or exact-route drift fails the all-or-nothing composition.
pub fn load_core_private_life_content(root: &Path) -> Result<CorePrivateLifeContent> {
    let world_flow = load_core_development_world_flow(root)
        .context("private-life content requires the validated Hall and Core micro-realm")?;
    let encounter_rooms = load_core_development_encounter_rooms(root)
        .context("private-life content requires the validated fixed Bell rooms")?;
    let caldus = load_core_development_caldus(root)
        .context("private-life content requires the validated Sir Caldus encounter")?;
    let hall_scene = world_flow
        .compile_hall_scene()
        .context("private-life Hall scene compilation failed")?;
    let microrealm_scene = world_flow
        .compile_microrealm_scene()
        .context("private-life micro-realm scene compilation failed")?;
    let fixed_layout = encounter_rooms
        .compile_fixed_layout_definition()
        .context("private-life fixed Bell layout compilation failed")?;
    validate_exact_route(
        &world_flow,
        &encounter_rooms,
        &caldus,
        &hall_scene,
        &microrealm_scene,
        &fixed_layout,
    )?;
    let revision = compose_revision(
        world_flow.hashes(),
        encounter_rooms.hashes(),
        caldus.hashes(),
    );
    Ok(CorePrivateLifeContent {
        world_flow,
        encounter_rooms,
        caldus,
        hall_scene,
        microrealm_scene,
        fixed_layout,
        revision,
    })
}

fn validate_exact_route(
    world_flow: &CoreDevelopmentWorldFlow,
    encounters: &CoreDevelopmentEncounterRooms,
    caldus: &CoreDevelopmentCaldus,
    hall_scene: &WorldSceneDefinition,
    microrealm_scene: &WorldSceneDefinition,
    fixed_layout: &FixedDungeonLayoutDefinition,
) -> Result<()> {
    let node_ids = fixed_layout
        .rooms
        .iter()
        .map(|room| room.node_id.as_str())
        .collect::<Vec<_>>();
    let expected_nodes = ["B0", "B1", "B2", "B3", "B4", "B5", "B6"];
    let disabled = fixed_layout
        .disabled_branch_node_ids
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    if world_flow.hub().header.id.as_str() != "hub.lantern_halls_01"
        || world_flow.world().header.id.as_str() != "world.core_microrealm_01"
        || world_flow.world().capacity != 1
        || encounters.pack_bell_01().header.id.as_str() != "pack.bell.01"
        || encounters.fixed_layout().header.id.as_str() != "layout.core_private_life_01"
        || hall_scene.id != "hub.lantern_halls_01"
        || microrealm_scene.id != "world.core_microrealm_01"
        || microrealm_scene.capacity != Some(1)
        || fixed_layout.id != "layout.core_private_life_01"
        || node_ids != expected_nodes
        || disabled != ["BB1", "BS1"]
        || caldus.boss().header.id.as_str() != "boss.sir_caldus"
        || caldus.room_binding().layout_id.as_str() != "layout.core_private_life_01"
        || caldus.room_binding().node_id != "B6"
        || caldus.room_binding().boss_id.as_str() != "boss.sir_caldus"
        || caldus.room_binding().exit_id != caldus.exit().header.id
    {
        bail!("Core private-life content no longer describes the exact M03 route");
    }
    Ok(())
}

fn compose_revision(
    world: &CoreWorldFlowHashes,
    encounters: &CoreEncounterRoomHashes,
    caldus: &CoreCaldusHashes,
) -> CorePrivateLifeContentRevision {
    CorePrivateLifeContentRevision {
        records_blake3: aggregate_channel(
            PRIVATE_LIFE_RECORDS_DOMAIN,
            [
                ("world_flow", world.records_blake3.as_str()),
                ("encounter_rooms", encounters.records_blake3.as_str()),
                ("caldus", caldus.records_blake3.as_str()),
            ],
        ),
        assets_blake3: aggregate_channel(
            PRIVATE_LIFE_ASSETS_DOMAIN,
            [
                ("world_flow", world.assets_blake3.as_str()),
                ("encounter_rooms", encounters.assets_blake3.as_str()),
                ("caldus", caldus.assets_blake3.as_str()),
            ],
        ),
        localization_blake3: aggregate_channel(
            PRIVATE_LIFE_LOCALIZATION_DOMAIN,
            [
                ("world_flow", world.localization_blake3.as_str()),
                ("encounter_rooms", encounters.localization_blake3.as_str()),
                ("caldus", caldus.localization_blake3.as_str()),
            ],
        ),
    }
}

fn aggregate_channel<'a>(
    domain: &[u8],
    components: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    for (name, revision) in components {
        update_length_prefixed(&mut hasher, name.as_bytes());
        update_length_prefixed(&mut hasher, revision.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn update_length_prefixed(hasher: &mut blake3::Hasher, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("route revision component length fits u64");
    hasher.update(&length.to_le_bytes());
    hasher.update(value);
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    #[test]
    fn checked_in_private_life_bundle_is_exact_and_revisioned() {
        let content =
            load_core_private_life_content(&content_root()).expect("private-life content");
        assert_eq!(content.hall_scene().id, "hub.lantern_halls_01");
        assert_eq!(content.microrealm_scene().capacity, Some(1));
        assert_eq!(content.fixed_layout().id, "layout.core_private_life_01");
        assert_eq!(
            content
                .fixed_layout()
                .rooms
                .iter()
                .map(|room| room.node_id.as_str())
                .collect::<Vec<_>>(),
            ["B0", "B1", "B2", "B3", "B4", "B5", "B6"]
        );
        assert_eq!(
            content.caldus().boss().header.id.as_str(),
            "boss.sir_caldus"
        );
        for hash in [
            &content.revision().records_blake3,
            &content.revision().assets_blake3,
            &content.revision().localization_blake3,
        ] {
            assert_eq!(hash.len(), 64);
            assert!(!hash.bytes().all(|byte| byte == b'0'));
        }
    }

    #[test]
    fn every_component_hash_perturbs_its_composed_route_channel() {
        let content =
            load_core_private_life_content(&content_root()).expect("private-life content");
        let baseline = content.revision().clone();
        let world = content.world_flow().hashes().clone();
        let encounters = content.encounter_rooms().hashes().clone();
        let caldus = content.caldus().hashes().clone();

        for index in 0..9 {
            let mut changed_world = world.clone();
            let mut changed_encounters = encounters.clone();
            let mut changed_caldus = caldus.clone();
            match index {
                0 => changed_world.records_blake3 = "1".repeat(64),
                1 => changed_encounters.records_blake3 = "2".repeat(64),
                2 => changed_caldus.records_blake3 = "3".repeat(64),
                3 => changed_world.assets_blake3 = "4".repeat(64),
                4 => changed_encounters.assets_blake3 = "5".repeat(64),
                5 => changed_caldus.assets_blake3 = "6".repeat(64),
                6 => changed_world.localization_blake3 = "7".repeat(64),
                7 => changed_encounters.localization_blake3 = "8".repeat(64),
                8 => changed_caldus.localization_blake3 = "9".repeat(64),
                _ => unreachable!(),
            }
            let changed = compose_revision(&changed_world, &changed_encounters, &changed_caldus);
            match index {
                0..=2 => assert_ne!(changed.records_blake3, baseline.records_blake3),
                3..=5 => assert_ne!(changed.assets_blake3, baseline.assets_blake3),
                6..=8 => assert_ne!(changed.localization_blake3, baseline.localization_blake3),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn aggregate_is_domain_separated_ordered_and_length_prefixed() {
        let first = aggregate_channel(
            PRIVATE_LIFE_RECORDS_DOMAIN,
            [("world_flow", "ab"), ("encounter_rooms", "c")],
        );
        let ambiguous_without_lengths = aggregate_channel(
            PRIVATE_LIFE_RECORDS_DOMAIN,
            [("world_flow", "a"), ("encounter_rooms", "bc")],
        );
        let reordered = aggregate_channel(
            PRIVATE_LIFE_RECORDS_DOMAIN,
            [("encounter_rooms", "c"), ("world_flow", "ab")],
        );
        let other_domain = aggregate_channel(
            PRIVATE_LIFE_ASSETS_DOMAIN,
            [("world_flow", "ab"), ("encounter_rooms", "c")],
        );
        assert_ne!(first, ambiguous_without_lengths);
        assert_ne!(first, reordered);
        assert_ne!(first, other_domain);
    }
}
