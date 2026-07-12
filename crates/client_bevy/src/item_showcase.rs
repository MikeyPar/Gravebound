use anyhow::{Context, Result};
use bevy::prelude::*;
use sim_content::{PrototypeEquipmentCatalog, PrototypeRewardCatalog, PrototypeRewardGrant};

use crate::{
    FrameSet,
    combat::{CollisionDiagnostics, EvidenceScenario},
    consumable::ConsumableDiagnostics,
    enemies::EnemyLabRuntime,
    player::PlayerSimulation,
};

#[derive(Debug, Resource)]
pub(crate) struct ItemShowcaseCatalog {
    equipment: PrototypeEquipmentCatalog,
    rewards: PrototypeRewardCatalog,
    content_version: String,
    boss_golden: Vec<PrototypeRewardGrant>,
}

impl ItemShowcaseCatalog {
    pub(crate) fn new(
        equipment: PrototypeEquipmentCatalog,
        rewards: PrototypeRewardCatalog,
        content_version: String,
    ) -> Result<Self> {
        let boss_golden = rewards
            .resolve("reward.prototype.boss", &content_version, 0xB311_A501, 7)
            .context("failed to resolve item-showcase boss golden")?;
        Ok(Self {
            equipment,
            rewards,
            content_version,
            boss_golden,
        })
    }

    pub(crate) fn evidence_ready(
        &self,
        runtime: &EnemyLabRuntime,
        player: &PlayerSimulation,
        consumable: &ConsumableDiagnostics,
    ) -> bool {
        self.equipment.items().len() == 12
            && self.rewards.tables().len() == 5
            && runtime.combat().weapon().projectile_count() == 3
            && runtime.combat().focused()
            && runtime
                .consumables()
                .definition()
                .restore_max_health_basis_points()
                == 3_500
            && runtime.consumables().definition().shared_cooldown_ticks() == 75
            && runtime.consumables().vitals().maximum_health() == 140
            && runtime.target_armor() == 2
            && (player.state().config().final_speed_tiles_per_second - 4.998).abs() < f32::EPSILON
            && consumable.showcase_ready()
    }

    pub(crate) fn resolve_reward(
        &self,
        table_id: &str,
        seed: u64,
        resolution_id: u64,
    ) -> Result<Vec<PrototypeRewardGrant>> {
        self.rewards
            .resolve(table_id, &self.content_version, seed, resolution_id)
    }

    pub(crate) fn equipment_slot(&self, content_id: &str) -> Option<sim_core::EquipmentSlot> {
        self.equipment.get(content_id)?;
        if content_id.starts_with("item.prototype.weapon.") {
            Some(sim_core::EquipmentSlot::Weapon)
        } else if content_id.starts_with("item.prototype.relic.") {
            Some(sim_core::EquipmentSlot::Relic)
        } else if content_id.starts_with("item.prototype.armor.") {
            Some(sim_core::EquipmentSlot::Armor)
        } else if content_id.starts_with("item.prototype.charm.") {
            Some(sim_core::EquipmentSlot::Charm)
        } else {
            None
        }
    }
}

#[derive(Debug, Component)]
struct ItemShowcaseOverlay;

pub(crate) fn configure(app: &mut App) {
    app.add_systems(Startup, spawn_overlay)
        .add_systems(Update, update_overlay.in_set(FrameSet::Presentation));
}

#[allow(clippy::needless_pass_by_value)]
fn spawn_overlay(mut commands: Commands, scenario: Res<EvidenceScenario>) {
    if *scenario != EvidenceScenario::ItemCatalogShowcase {
        return;
    }
    commands.spawn((
        Name::new("Prototype item behavior matrix"),
        ItemShowcaseOverlay,
        Text::new("07B ITEM MATRIX"),
        TextFont::from_font_size(13.0),
        TextColor(Color::srgb_u8(237, 227, 197)),
        Node {
            position_type: PositionType::Absolute,
            right: px(14),
            bottom: px(24),
            width: px(505),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(10, 11, 15, 238)),
        BorderColor::all(Color::srgba_u8(214, 164, 78, 225)),
    ));
}

#[allow(clippy::needless_pass_by_value)]
fn update_overlay(
    catalog: Res<ItemShowcaseCatalog>,
    runtime: Res<EnemyLabRuntime>,
    player: Res<PlayerSimulation>,
    collision: Res<CollisionDiagnostics>,
    mut text: Single<&mut Text, With<ItemShowcaseOverlay>>,
) {
    let tonic = runtime.consumables();
    let golden = catalog
        .boss_golden
        .iter()
        .map(|grant| format!("{}x{}", short_id(&grant.item_id), grant.quantity))
        .collect::<Vec<_>>()
        .join(" / ");
    text.0 = format!(
        "07B LIVE ITEM MATRIX | CATALOG {}/12 | REWARDS {}/5 | {}\nSCATTERBOW 3x12 | +/-8 DEG | 16T | RANGE 8 | TARGET CAP 2 | BOLTS {}\nSTILL EYE FOCUS 12T | +6% DAMAGE | +10% SPEED | FOCUSED {} | RAW13 CONTACTS {}\nPARISH LEATHER HP {}/140 | ARMOR {} | MOVE {:.3} (x0.98)\nUNDERTAKER TONIC 35% / 12T | COOLDOWN {}/75T | BELT x{}\nDEFAULT-SEED BOSS: {} | DISTINCT 3 + TONICS 2",
        catalog.equipment.items().len(),
        catalog.rewards.tables().len(),
        catalog.content_version,
        runtime.combat().projectiles().len(),
        runtime.combat().focused(),
        collision.focused_raw_intents(),
        tonic.vitals().current_health(),
        runtime.target_armor(),
        player.state().config().final_speed_tiles_per_second,
        tonic.shared_cooldown_remaining_ticks(),
        tonic.belt().slots()[0].tonic_count(),
        golden,
    );
}

fn short_id(content_id: &str) -> &str {
    content_id.rsplit('.').next().unwrap_or(content_id)
}
