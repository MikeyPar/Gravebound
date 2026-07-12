use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use content_schema::{
    DropRollGroup, EffectOperation, EquipmentSlot, ItemEffect, ItemPayload, ItemRarity, ItemRecord,
};
use sim_core::{
    DeterministicRng, WeaponDefinition, WeaponDefinitionParameters, duration_ms_to_ticks_nearest,
};

use crate::ContentPackage;

pub const FIRST_PLAYABLE_EQUIPMENT_COUNT: usize = 12;
pub const FIRST_PLAYABLE_REWARD_TABLE_COUNT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrototypeItemBehavior {
    Crossbow {
        damage_per_bolt: u32,
        displayed_single_target_damage: u32,
        interval_ticks: u32,
        range_milli_tiles: u32,
        speed_milli_tiles_per_second: u32,
        radius_milli_tiles: u32,
        bolt_angles_milli_degrees: Vec<i32>,
        max_bolts_per_target: u32,
        pierce: u32,
    },
    GraveMark {
        range_milli_tiles: Option<u32>,
        duration_ticks: Option<u32>,
        primary_bonus_basis_points: Option<u32>,
    },
    Slipstep {
        cooldown_ticks: u32,
        empowered_window_ticks: u32,
    },
    Armor {
        max_health_add: i32,
        max_health_multiplier_basis_points: u32,
        armor_add: i32,
        movement_multiplier_basis_points: u32,
        veil_resistance_add_basis_points: i32,
    },
    StillEye {
        activation_ticks: u32,
        focused_damage_bonus_basis_points: u32,
        focused_speed_bonus_basis_points: u32,
    },
    UndertakerKnot {
        tonic_restore_basis_points: u32,
        shared_cooldown_ticks: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrototypeEquipmentDefinition {
    pub content_id: String,
    pub slot: EquipmentSlot,
    pub rarity: ItemRarity,
    pub behavior: PrototypeItemBehavior,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrototypeEquipmentCatalog {
    items: BTreeMap<String, PrototypeEquipmentDefinition>,
}

impl PrototypeEquipmentCatalog {
    #[must_use]
    pub fn items(&self) -> &BTreeMap<String, PrototypeEquipmentDefinition> {
        &self.items
    }

    #[must_use]
    pub fn get(&self, content_id: &str) -> Option<&PrototypeEquipmentDefinition> {
        self.items.get(content_id)
    }

    pub fn crossbow(&self, content_id: &str) -> Result<WeaponDefinition> {
        let item = self
            .get(content_id)
            .with_context(|| format!("unknown prototype equipment `{content_id}`"))?;
        let PrototypeItemBehavior::Crossbow {
            damage_per_bolt,
            interval_ticks,
            range_milli_tiles,
            speed_milli_tiles_per_second,
            radius_milli_tiles,
            bolt_angles_milli_degrees,
            max_bolts_per_target,
            pierce,
            ..
        } = &item.behavior
        else {
            bail!("{content_id} is not a prototype crossbow");
        };
        let directions = bolt_angles_milli_degrees
            .iter()
            .map(|angle| match *angle {
                -8_000 => Ok((990_268, -139_173)),
                0 => Ok((1_000_000, 0)),
                8_000 => Ok((990_268, 139_173)),
                _ => bail!("{content_id} uses an unsupported FP bolt angle {angle}"),
            })
            .collect::<Result<Vec<_>>>()?;
        WeaponDefinition::new(WeaponDefinitionParameters {
            content_id: item.content_id.clone(),
            raw_damage: *damage_per_bolt,
            attack_interval_ticks: *interval_ticks,
            range_milli_tiles: *range_milli_tiles,
            projectile_speed_milli_tiles_per_second: *speed_milli_tiles_per_second,
            projectile_radius_milli_tiles: *radius_milli_tiles,
            projectile_count: u32::try_from(directions.len())
                .context("prototype bolt count exceeds u32")?,
            projectile_directions_millionths: directions,
            max_projectiles_per_target: *max_bolts_per_target,
            pierce: *pierce,
            stops_on_first_enemy: true,
        })
        .with_context(|| format!("{content_id} failed simulation weapon validation"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledRewardOutcome {
    pub item_id: String,
    pub weight: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledRewardGroup {
    pub group_id: String,
    pub presence_basis_points: u32,
    pub selections: u32,
    pub without_replacement: bool,
    pub outcomes: Vec<CompiledRewardOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrototypeRewardTable {
    pub content_id: String,
    pub groups: Vec<CompiledRewardGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrototypeRewardCatalog {
    tables: BTreeMap<String, PrototypeRewardTable>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrototypeRewardGrant {
    pub item_id: String,
    pub quantity: u32,
}

impl PrototypeRewardCatalog {
    #[must_use]
    pub fn tables(&self) -> &BTreeMap<String, PrototypeRewardTable> {
        &self.tables
    }

    pub fn resolve(
        &self,
        table_id: &str,
        content_version: &str,
        root_seed: u64,
        resolution_id: u64,
    ) -> Result<Vec<PrototypeRewardGrant>> {
        let table = self
            .tables
            .get(table_id)
            .with_context(|| format!("unknown prototype reward table `{table_id}`"))?;
        let label = format!("reward:{table_id}:{resolution_id}");
        let mut rng = DeterministicRng::new(content_version, root_seed, &label);
        let mut selected_equipment = BTreeSet::new();
        let mut grants: Vec<PrototypeRewardGrant> = Vec::new();
        for group in &table.groups {
            let present = match group.presence_basis_points {
                0 => false,
                10_000 => true,
                threshold => rng.bounded_u64(10_000)? < u64::from(threshold),
            };
            if !present {
                continue;
            }
            for _ in 0..group.selections {
                let candidates = group
                    .outcomes
                    .iter()
                    .filter(|outcome| {
                        !group.without_replacement || !selected_equipment.contains(&outcome.item_id)
                    })
                    .collect::<Vec<_>>();
                let total = candidates.iter().try_fold(0_u64, |sum, outcome| {
                    sum.checked_add(u64::from(outcome.weight))
                        .context("prototype reward weight overflow")
                })?;
                if total == 0 {
                    bail!("prototype reward group exhausted before all selections");
                }
                let mut draw = rng.bounded_u64(total)?;
                let selected = candidates
                    .into_iter()
                    .find(|outcome| {
                        if draw < u64::from(outcome.weight) {
                            true
                        } else {
                            draw -= u64::from(outcome.weight);
                            false
                        }
                    })
                    .context("weighted reward draw did not resolve")?;
                if selected.item_id.starts_with("item.prototype.") {
                    selected_equipment.insert(selected.item_id.clone());
                }
                if let Some(existing) = grants
                    .iter_mut()
                    .find(|grant| grant.item_id == selected.item_id)
                {
                    existing.quantity = existing
                        .quantity
                        .checked_add(1)
                        .context("prototype reward quantity overflow")?;
                } else {
                    grants.push(PrototypeRewardGrant {
                        item_id: selected.item_id.clone(),
                        quantity: 1,
                    });
                }
            }
        }
        Ok(grants)
    }
}

pub fn first_playable_equipment_catalog(
    package: &ContentPackage,
) -> Result<PrototypeEquipmentCatalog> {
    let mut items = BTreeMap::new();
    for record in package
        .items
        .iter()
        .filter(|record| record.header.id.as_str().starts_with("item.prototype."))
    {
        let compiled = compile_exact_item(record)?;
        if items
            .insert(compiled.content_id.clone(), compiled)
            .is_some()
        {
            bail!("duplicate prototype equipment ID {}", record.header.id);
        }
    }
    if items.len() != FIRST_PLAYABLE_EQUIPMENT_COUNT {
        bail!(
            "expected {FIRST_PLAYABLE_EQUIPMENT_COUNT} prototype equipment definitions, found {}",
            items.len()
        );
    }
    Ok(PrototypeEquipmentCatalog { items })
}

pub fn first_playable_reward_catalog(package: &ContentPackage) -> Result<PrototypeRewardCatalog> {
    let mut tables = BTreeMap::new();
    for record in &package.drop_tables {
        let groups = record
            .numeric_payload
            .roll_groups
            .iter()
            .map(compile_reward_group)
            .collect();
        let table = PrototypeRewardTable {
            content_id: record.header.id.to_string(),
            groups,
        };
        validate_exact_reward_table(&table)?;
        if tables.insert(table.content_id.clone(), table).is_some() {
            bail!("duplicate prototype reward table ID {}", record.header.id);
        }
    }
    if tables.len() != FIRST_PLAYABLE_REWARD_TABLE_COUNT {
        bail!(
            "expected {FIRST_PLAYABLE_REWARD_TABLE_COUNT} prototype reward tables, found {}",
            tables.len()
        );
    }
    Ok(PrototypeRewardCatalog { tables })
}

fn compile_reward_group(group: &DropRollGroup) -> CompiledRewardGroup {
    CompiledRewardGroup {
        group_id: group.group_id.clone(),
        presence_basis_points: group.presence_basis_points,
        selections: group.selections,
        without_replacement: group.without_replacement,
        outcomes: group
            .outcomes
            .iter()
            .map(|outcome| CompiledRewardOutcome {
                item_id: outcome.item_id.to_string(),
                weight: outcome.weight,
            })
            .collect(),
    }
}

#[derive(Clone, Copy)]
struct ExpectedEffect {
    stat: &'static str,
    operation: EffectOperation,
    value: i32,
}

fn exact_effects(record: &ItemRecord, expected: &[ExpectedEffect]) -> Result<()> {
    let ItemPayload::Equipment { effects, .. } = &record.numeric_payload else {
        bail!("{} is not equipment", record.header.id);
    };
    if effects.len() != expected.len() {
        bail!("{} has the wrong effect count", record.header.id);
    }
    for expected in expected {
        let matches = effects
            .iter()
            .filter(|effect| effect.stat == expected.stat)
            .collect::<Vec<&ItemEffect>>();
        if matches.len() != 1
            || matches[0].operation != expected.operation
            || matches[0].value != expected.value
        {
            bail!(
                "{} has invalid `{}` behavior",
                record.header.id,
                expected.stat
            );
        }
    }
    Ok(())
}

fn compile_exact_item(record: &ItemRecord) -> Result<PrototypeEquipmentDefinition> {
    let ItemPayload::Equipment { slot, rarity, .. } = &record.numeric_payload else {
        bail!("{} is not equipment", record.header.id);
    };
    let id = record.header.id.as_str();
    let (expected_slot, expected_rarity, expected, behavior) = exact_item_contract(id)?;
    if *slot != expected_slot || *rarity != expected_rarity {
        bail!("{id} has an invalid slot or rarity");
    }
    exact_effects(record, &expected)?;
    Ok(PrototypeEquipmentDefinition {
        content_id: id.to_owned(),
        slot: *slot,
        rarity: *rarity,
        behavior,
    })
}

type ItemContract = (
    EquipmentSlot,
    ItemRarity,
    Vec<ExpectedEffect>,
    PrototypeItemBehavior,
);

#[allow(clippy::too_many_lines)]
fn exact_item_contract(id: &str) -> Result<ItemContract> {
    use EffectOperation::{Add, MultiplyBasisPoints, Set};
    let effect = |stat, operation, value| ExpectedEffect {
        stat,
        operation,
        value,
    };
    let crossbow = |damage, displayed, interval_ms, range, speed, radius, angles, max_hits| {
        PrototypeItemBehavior::Crossbow {
            damage_per_bolt: damage,
            displayed_single_target_damage: displayed,
            interval_ticks: u32::try_from(duration_ms_to_ticks_nearest(interval_ms))
                .expect("FP interval"),
            range_milli_tiles: range,
            speed_milli_tiles_per_second: speed,
            radius_milli_tiles: radius,
            bolt_angles_milli_degrees: angles,
            max_bolts_per_target: max_hits,
            pierce: 0,
        }
    };
    Ok(match id {
        "item.prototype.weapon.pine_crossbow" => (
            EquipmentSlot::Weapon,
            ItemRarity::Worn,
            vec![
                effect("primary_damage", Set, 20),
                effect("attack_interval_ms", Set, 455),
                effect("range_milli_tiles", Set, 9500),
                effect("projectile_speed_milli_tiles_per_second", Set, 12000),
                effect("projectile_radius_milli_tiles", Set, 100),
                effect("projectile_count", Set, 1),
                effect("pierce", Set, 0),
            ],
            crossbow(20, 20, 455, 9500, 12000, 100, vec![0], 1),
        ),
        "item.prototype.weapon.grave_repeater" => (
            EquipmentSlot::Weapon,
            ItemRarity::Forged,
            vec![
                effect("primary_damage", Set, 17),
                effect("attack_interval_ms", Set, 360),
                effect("range_milli_tiles", Set, 8500),
                effect("projectile_speed_milli_tiles_per_second", Set, 11000),
                effect("projectile_radius_milli_tiles", Set, 100),
                effect("projectile_count", Set, 1),
                effect("pierce", Set, 0),
            ],
            crossbow(17, 17, 360, 8500, 11000, 100, vec![0], 1),
        ),
        "item.prototype.weapon.longbolt_crossbow" => (
            EquipmentSlot::Weapon,
            ItemRarity::Oathed,
            vec![
                effect("primary_damage", Set, 28),
                effect("attack_interval_ms", Set, 600),
                effect("range_milli_tiles", Set, 12000),
                effect("projectile_speed_milli_tiles_per_second", Set, 15000),
                effect("projectile_radius_milli_tiles", Set, 90),
                effect("projectile_count", Set, 1),
                effect("pierce", Set, 0),
            ],
            crossbow(28, 28, 600, 12000, 15000, 90, vec![0], 1),
        ),
        "item.prototype.weapon.scatterbow" => (
            EquipmentSlot::Weapon,
            ItemRarity::Relic,
            vec![
                effect("primary_damage_per_bolt", Set, 12),
                effect("attack_interval_ms", Set, 520),
                effect("range_milli_tiles", Set, 8000),
                effect("projectile_speed_milli_tiles_per_second", Set, 10500),
                effect("projectile_radius_milli_tiles", Set, 100),
                effect("projectile_count", Set, 3),
                effect("spread_degrees", Set, 8),
                effect("max_bolts_per_target", Set, 2),
            ],
            crossbow(12, 24, 520, 8000, 10500, 100, vec![-8000, 0, 8000], 2),
        ),
        "item.prototype.relic.dented_scope" => (
            EquipmentSlot::Relic,
            ItemRarity::Worn,
            vec![effect("grave_mark_range_milli_tiles", Set, 12000)],
            PrototypeItemBehavior::GraveMark {
                range_milli_tiles: Some(12000),
                duration_ticks: None,
                primary_bonus_basis_points: None,
            },
        ),
        "item.prototype.relic.mark_lens" => (
            EquipmentSlot::Relic,
            ItemRarity::Oathed,
            vec![
                effect("grave_mark_duration_ms", Set, 6000),
                effect("grave_mark_primary_bonus_basis_points", Set, 1200),
            ],
            PrototypeItemBehavior::GraveMark {
                range_milli_tiles: None,
                duration_ticks: Some(180),
                primary_bonus_basis_points: Some(1200),
            },
        ),
        "item.prototype.relic.slip_clasp" => (
            EquipmentSlot::Relic,
            ItemRarity::Oathed,
            vec![
                effect("slipstep_cooldown_ms", Set, 7000),
                effect("slipstep_empowered_window_ms", Set, 1000),
            ],
            PrototypeItemBehavior::Slipstep {
                cooldown_ticks: 210,
                empowered_window_ticks: 30,
            },
        ),
        "item.prototype.armor.reedcloth_wraps" => (
            EquipmentSlot::Armor,
            ItemRarity::Worn,
            vec![effect("max_health", Add, 8)],
            PrototypeItemBehavior::Armor {
                max_health_add: 8,
                max_health_multiplier_basis_points: 10000,
                armor_add: 0,
                movement_multiplier_basis_points: 10000,
                veil_resistance_add_basis_points: 0,
            },
        ),
        "item.prototype.armor.parish_leather" => (
            EquipmentSlot::Armor,
            ItemRarity::Forged,
            vec![
                effect("max_health", Add, 20),
                effect("armor", Add, 2),
                effect("movement_speed", MultiplyBasisPoints, 9800),
            ],
            PrototypeItemBehavior::Armor {
                max_health_add: 20,
                max_health_multiplier_basis_points: 10000,
                armor_add: 2,
                movement_multiplier_basis_points: 9800,
                veil_resistance_add_basis_points: 0,
            },
        ),
        "item.prototype.armor.saltglass_coat" => (
            EquipmentSlot::Armor,
            ItemRarity::Oathed,
            vec![
                effect("max_health", MultiplyBasisPoints, 9200),
                effect("armor", Add, 1),
                effect("veil_resistance_basis_points", Add, 1200),
            ],
            PrototypeItemBehavior::Armor {
                max_health_add: 0,
                max_health_multiplier_basis_points: 9200,
                armor_add: 1,
                movement_multiplier_basis_points: 10000,
                veil_resistance_add_basis_points: 1200,
            },
        ),
        "item.prototype.charm.still_eye" => (
            EquipmentSlot::Charm,
            ItemRarity::Oathed,
            vec![
                effect("stillness_activation_ms", Set, 400),
                effect("focused_primary_damage_bonus_basis_points", Set, 600),
            ],
            PrototypeItemBehavior::StillEye {
                activation_ticks: 12,
                focused_damage_bonus_basis_points: 600,
                focused_speed_bonus_basis_points: 1000,
            },
        ),
        "item.prototype.charm.undertaker_knot" => (
            EquipmentSlot::Charm,
            ItemRarity::Oathed,
            vec![
                effect("red_tonic_restore_basis_points", Set, 3500),
                effect("shared_potion_cooldown_ms", Set, 2500),
            ],
            PrototypeItemBehavior::UndertakerKnot {
                tonic_restore_basis_points: 3500,
                shared_cooldown_ticks: 75,
            },
        ),
        _ => bail!("unexpected prototype equipment ID `{id}`"),
    })
}

#[allow(clippy::too_many_lines)] // One explicit contract keeps all five authored tables reviewable.
fn validate_exact_reward_table(table: &PrototypeRewardTable) -> Result<()> {
    let group = |index: usize,
                 id: &str,
                 presence,
                 selections,
                 without_replacement,
                 outcomes: &[(&str, u32)]|
     -> Result<()> {
        let actual = table.groups.get(index).context("missing reward group")?;
        if actual.group_id != id
            || actual.presence_basis_points != presence
            || actual.selections != selections
            || actual.without_replacement != without_replacement
            || actual.outcomes.len() != outcomes.len()
        {
            bail!("{} group `{id}` has invalid metadata", table.content_id);
        }
        for (actual, (item_id, weight)) in actual.outcomes.iter().zip(outcomes) {
            if actual.item_id != *item_id || actual.weight != *weight {
                bail!("{} group `{id}` has invalid outcomes", table.content_id);
            }
        }
        Ok(())
    };
    let global = [
        ("item.prototype.weapon.pine_crossbow", 12),
        ("item.prototype.weapon.grave_repeater", 10),
        ("item.prototype.weapon.longbolt_crossbow", 6),
        ("item.prototype.weapon.scatterbow", 6),
        ("item.prototype.relic.dented_scope", 12),
        ("item.prototype.relic.mark_lens", 8),
        ("item.prototype.relic.slip_clasp", 8),
        ("item.prototype.armor.reedcloth_wraps", 12),
        ("item.prototype.armor.parish_leather", 10),
        ("item.prototype.armor.saltglass_coat", 6),
        ("item.prototype.charm.still_eye", 6),
        ("item.prototype.charm.undertaker_knot", 4),
    ];
    match table.content_id.as_str() {
        "reward.prototype.normal_enemy" => {
            group(0, "global_equipment", 800, 1, false, &global)?;
            group(
                1,
                "red_tonic",
                1000,
                1,
                false,
                &[("consumable.red_tonic", 1)],
            )?;
        }
        "reward.prototype.wave_1" => {
            group(
                0,
                "weapon",
                10000,
                1,
                false,
                &[
                    ("item.prototype.weapon.pine_crossbow", 35),
                    ("item.prototype.weapon.grave_repeater", 30),
                    ("item.prototype.weapon.longbolt_crossbow", 20),
                    ("item.prototype.weapon.scatterbow", 15),
                ],
            )?;
            group(1, "tonic", 10000, 1, false, &[("consumable.red_tonic", 1)])?;
        }
        "reward.prototype.wave_2" => {
            group(
                0,
                "relic",
                10000,
                1,
                false,
                &[
                    ("item.prototype.relic.dented_scope", 40),
                    ("item.prototype.relic.mark_lens", 30),
                    ("item.prototype.relic.slip_clasp", 30),
                ],
            )?;
            group(
                1,
                "armor",
                10000,
                1,
                false,
                &[
                    ("item.prototype.armor.reedcloth_wraps", 40),
                    ("item.prototype.armor.parish_leather", 35),
                    ("item.prototype.armor.saltglass_coat", 25),
                ],
            )?;
        }
        "reward.prototype.wave_3" => {
            group(
                0,
                "charm",
                10000,
                1,
                false,
                &[
                    ("item.prototype.charm.still_eye", 60),
                    ("item.prototype.charm.undertaker_knot", 40),
                ],
            )?;
            group(1, "global_equipment", 10000, 1, true, &global)?;
        }
        "reward.prototype.boss" => {
            group(0, "global_equipment", 10000, 3, true, &global)?;
            group(1, "tonics", 10000, 2, false, &[("consumable.red_tonic", 1)])?;
        }
        _ => bail!("unexpected prototype reward table `{}`", table.content_id),
    }
    if table.groups.len() != 2 {
        bail!("{} must contain exactly two groups", table.content_id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn package() -> ContentPackage {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content");
        crate::load_and_validate(&root).expect("valid content").0
    }

    #[test]
    fn all_twelve_items_compile_to_typed_exact_behaviors() {
        let catalog = first_playable_equipment_catalog(&package()).expect("catalog");
        assert_eq!(catalog.items().len(), FIRST_PLAYABLE_EQUIPMENT_COUNT);
        assert_eq!(
            catalog
                .get("item.prototype.weapon.scatterbow")
                .expect("scatterbow")
                .behavior,
            PrototypeItemBehavior::Crossbow {
                damage_per_bolt: 12,
                displayed_single_target_damage: 24,
                interval_ticks: 16,
                range_milli_tiles: 8_000,
                speed_milli_tiles_per_second: 10_500,
                radius_milli_tiles: 100,
                bolt_angles_milli_degrees: vec![-8_000, 0, 8_000],
                max_bolts_per_target: 2,
                pierce: 0,
            }
        );
        let scatter = catalog
            .crossbow("item.prototype.weapon.scatterbow")
            .expect("compiled Scatterbow");
        assert_eq!(scatter.projectile_count(), 3);
        assert_eq!(scatter.max_projectiles_per_target(), 2);
        assert_eq!(
            catalog
                .get("item.prototype.armor.parish_leather")
                .expect("armor")
                .behavior,
            PrototypeItemBehavior::Armor {
                max_health_add: 20,
                max_health_multiplier_basis_points: 10_000,
                armor_add: 2,
                movement_multiplier_basis_points: 9_800,
                veil_resistance_add_basis_points: 0,
            }
        );
        assert_eq!(
            catalog
                .get("item.prototype.charm.still_eye")
                .expect("charm")
                .behavior,
            PrototypeItemBehavior::StillEye {
                activation_ticks: 12,
                focused_damage_bonus_basis_points: 600,
                focused_speed_bonus_basis_points: 1_000,
            }
        );
        assert_eq!(
            catalog
                .get("item.prototype.charm.undertaker_knot")
                .expect("charm")
                .behavior,
            PrototypeItemBehavior::UndertakerKnot {
                tonic_restore_basis_points: 3_500,
                shared_cooldown_ticks: 75,
            }
        );
    }

    #[test]
    fn any_item_effect_slot_rarity_or_id_drift_fails_catalog_compilation() {
        let base = package();
        for item_index in 0..FIRST_PLAYABLE_EQUIPMENT_COUNT {
            let ItemPayload::Equipment { effects, .. } = &base.items[item_index].numeric_payload
            else {
                panic!("equipment")
            };
            for effect_index in 0..effects.len() {
                let mut changed = base.clone();
                let ItemPayload::Equipment { effects, .. } =
                    &mut changed.items[item_index].numeric_payload
                else {
                    panic!("equipment")
                };
                effects[effect_index].value += 1;
                assert!(first_playable_equipment_catalog(&changed).is_err());
            }
            let mut changed = base.clone();
            let ItemPayload::Equipment { rarity, .. } =
                &mut changed.items[item_index].numeric_payload
            else {
                panic!("equipment")
            };
            *rarity = ItemRarity::Relic;
            if base.items[item_index].header.id.as_str() != "item.prototype.weapon.scatterbow" {
                assert!(first_playable_equipment_catalog(&changed).is_err());
            }
        }
        let mut changed = base;
        changed.items[0].header.id =
            content_schema::ContentId::parse("item.prototype.weapon.substitute").expect("valid ID");
        assert!(first_playable_equipment_catalog(&changed).is_err());
    }

    #[test]
    fn five_reward_tables_resolve_deterministically_with_cross_group_nonduplication() {
        let catalog = first_playable_reward_catalog(&package()).expect("catalog");
        assert_eq!(catalog.tables().len(), FIRST_PLAYABLE_REWARD_TABLE_COUNT);
        let expected = BTreeMap::from([
            (
                "reward.prototype.boss",
                vec![
                    ("item.prototype.weapon.pine_crossbow", 1),
                    ("item.prototype.armor.reedcloth_wraps", 1),
                    ("item.prototype.relic.dented_scope", 1),
                    ("consumable.red_tonic", 2),
                ],
            ),
            ("reward.prototype.normal_enemy", vec![]),
            (
                "reward.prototype.wave_1",
                vec![
                    ("item.prototype.weapon.grave_repeater", 1),
                    ("consumable.red_tonic", 1),
                ],
            ),
            (
                "reward.prototype.wave_2",
                vec![
                    ("item.prototype.relic.slip_clasp", 1),
                    ("item.prototype.armor.parish_leather", 1),
                ],
            ),
            (
                "reward.prototype.wave_3",
                vec![
                    ("item.prototype.charm.undertaker_knot", 1),
                    ("item.prototype.weapon.scatterbow", 1),
                ],
            ),
        ]);
        for table in catalog.tables().keys() {
            let first = catalog
                .resolve(table, "fp.1.0.0", 0xB311_A501, 7)
                .expect("resolve");
            let second = catalog
                .resolve(table, "fp.1.0.0", 0xB311_A501, 7)
                .expect("replay");
            assert_eq!(first, second);
            assert_eq!(
                first
                    .iter()
                    .map(|grant| (grant.item_id.as_str(), grant.quantity))
                    .collect::<Vec<_>>(),
                expected[table.as_str()]
            );
        }
        let wave_three = catalog
            .resolve("reward.prototype.wave_3", "fp.1.0.0", 0xB311_A501, 7)
            .expect("wave three");
        assert_eq!(wave_three.len(), 2);
        assert_ne!(wave_three[0].item_id, wave_three[1].item_id);
        let boss = catalog
            .resolve("reward.prototype.boss", "fp.1.0.0", 0xB311_A501, 7)
            .expect("boss");
        assert_eq!(
            boss.iter()
                .filter(|grant| grant.item_id.starts_with("item.prototype."))
                .count(),
            3
        );
        assert_eq!(
            boss.iter()
                .find(|grant| grant.item_id == "consumable.red_tonic")
                .expect("tonics")
                .quantity,
            2
        );
    }

    #[test]
    fn every_reward_payload_drift_fails_exact_compilation() {
        let base = package();
        for table_index in 0..base.drop_tables.len() {
            for group_index in 0..base.drop_tables[table_index]
                .numeric_payload
                .roll_groups
                .len()
            {
                let mut changed = base.clone();
                changed.drop_tables[table_index].numeric_payload.roll_groups[group_index]
                    .presence_basis_points ^= 1;
                assert!(first_playable_reward_catalog(&changed).is_err());
                let mut changed = base.clone();
                changed.drop_tables[table_index].numeric_payload.roll_groups[group_index]
                    .outcomes[0]
                    .weight += 1;
                assert!(first_playable_reward_catalog(&changed).is_err());
            }
        }
    }

    #[test]
    fn normal_enemy_independent_checks_can_produce_all_four_outcomes() {
        let catalog = first_playable_reward_catalog(&package()).expect("catalog");
        let mut shapes = BTreeSet::new();
        for resolution_id in 0..20_000 {
            let grants = catalog
                .resolve(
                    "reward.prototype.normal_enemy",
                    "fp.1.0.0",
                    0xB311_A501,
                    resolution_id,
                )
                .expect("resolve");
            shapes.insert((
                grants
                    .iter()
                    .any(|grant| grant.item_id.starts_with("item.prototype.")),
                grants
                    .iter()
                    .any(|grant| grant.item_id == "consumable.red_tonic"),
            ));
        }
        assert_eq!(
            shapes,
            BTreeSet::from([(false, false), (false, true), (true, false), (true, true)])
        );
    }
}
