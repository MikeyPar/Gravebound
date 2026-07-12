use bevy::prelude::*;
use sim_core::{
    EquipmentItem, EquipmentSlot, FieldPickup, FieldPickupAccess, FieldPickupId, InventoryStack,
    ItemContentId, ItemInstanceId, OwnedItemLocation, PlacementChoice, RewardChoice, RewardOutcome,
    SimulationVector,
};

use crate::{
    FixedSimulationSet, FrameSet, combat::EvidenceScenario, death::LocalDeathRuntime,
    enemies::EnemyLabRuntime, player::PlayerSimulation,
};

const INVENTORY_TOGGLE: KeyCode = KeyCode::KeyI;

#[derive(Debug, Default, Resource)]
pub(crate) struct InventoryDiagnostics {
    evidence_collected: bool,
    last_result: String,
    reward: Option<FieldPickup>,
    reward_result: String,
}

impl InventoryDiagnostics {
    pub(crate) fn evidence_ready(&self) -> bool {
        self.evidence_collected
            && self.reward.is_some()
            && self.reward_result == "LEAVE ACCEPTED | REWARD PRESERVED"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RewardAction {
    Take,
    Equip,
    DropArmorThenEquip,
    Leave,
}

impl RewardAction {
    fn from_keyboard(keyboard: &ButtonInput<KeyCode>) -> Option<Self> {
        [
            (KeyCode::Digit1, Self::Take),
            (KeyCode::Digit2, Self::Equip),
            (KeyCode::Digit3, Self::DropArmorThenEquip),
            (KeyCode::Digit4, Self::Leave),
        ]
        .into_iter()
        .find_map(|(key, action)| keyboard.just_pressed(key).then_some(action))
    }

    fn choice(self) -> RewardChoice {
        match self {
            Self::Take => RewardChoice::Take,
            Self::Equip => RewardChoice::Equip,
            Self::DropArmorThenEquip => RewardChoice::DropExisting {
                location: OwnedItemLocation::Equipped(EquipmentSlot::Armor),
                dropped_pickup_id: FieldPickupId::new(40_003).expect("nonzero drop ID"),
                then: PlacementChoice::Equip,
            },
            Self::Leave => RewardChoice::LeaveReward,
        }
    }
}

#[derive(Debug, Component)]
struct InventoryOverlay;

pub(crate) fn configure(app: &mut App) {
    app.init_resource::<InventoryDiagnostics>()
        .add_systems(Startup, spawn_inventory_overlay)
        .add_systems(
            FixedUpdate,
            (run_inventory_showcase, apply_reward_input)
                .chain()
                .in_set(FixedSimulationSet::Inventory),
        )
        .add_systems(
            Update,
            update_inventory_overlay.in_set(FrameSet::Presentation),
        );
}

#[allow(clippy::needless_pass_by_value)]
fn run_inventory_showcase(
    scenario: Res<EvidenceScenario>,
    runtime: Res<EnemyLabRuntime>,
    player: Res<PlayerSimulation>,
    mut death: ResMut<LocalDeathRuntime>,
    mut diagnostics: ResMut<InventoryDiagnostics>,
) {
    if *scenario != EvidenceScenario::InventoryShowcase || diagnostics.evidence_collected {
        return;
    }
    let now = runtime.combat().tick();
    let player_position = player.state().position();
    let pickup_position = player_position + SimulationVector::new(0.75, 0.0);
    let mut pickup = canonical_pickup(
        40_001,
        "item.prototype.charm.still_eye",
        EquipmentSlot::Charm,
        pickup_position,
        now,
    );
    let outcome = death
        .apply_field_pickup(
            &mut pickup,
            PlacementChoice::Take,
            player_position,
            FieldPickupAccess::Automatic,
            now,
        )
        .expect("validated field pickup");
    diagnostics.last_result = match outcome {
        sim_core::PickupOutcome::TakenToBackpack { backpack_index, .. } => format!(
            "FIELD TAKE ACCEPTED | STILL EYE -> BACKPACK {}",
            backpack_index + 1
        ),
        _ => format!("FIELD TAKE UNEXPECTED: {outcome:?}"),
    };
    diagnostics.evidence_collected =
        pickup.is_collected() && death.inventory().backpack_stack(0).is_some();
    diagnostics.reward = Some(canonical_pickup(
        40_002,
        "item.prototype.armor.parish_leather",
        EquipmentSlot::Armor,
        player_position,
        now,
    ));
}

fn canonical_pickup(
    id: u64,
    content_id: &str,
    slot: EquipmentSlot,
    position: SimulationVector,
    now: sim_core::Tick,
) -> FieldPickup {
    FieldPickup::new(
        FieldPickupId::new(id).expect("nonzero pickup ID"),
        InventoryStack::Equipment(EquipmentItem::new(
            ItemInstanceId::new(id).expect("nonzero item ID"),
            ItemContentId::new(content_id).expect("canonical prototype item ID"),
            slot,
        )),
        position,
        now,
    )
    .expect("canonical prototype pickup")
}

#[allow(clippy::needless_pass_by_value)]
fn apply_reward_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    scenario: Res<EvidenceScenario>,
    runtime: Res<EnemyLabRuntime>,
    mut death: ResMut<LocalDeathRuntime>,
    mut diagnostics: ResMut<InventoryDiagnostics>,
) {
    let scripted_leave = *scenario == EvidenceScenario::InventoryShowcase
        && diagnostics.reward_result.is_empty()
        && diagnostics.reward.is_some();
    let Some(action) =
        RewardAction::from_keyboard(&keyboard).or(scripted_leave.then_some(RewardAction::Leave))
    else {
        return;
    };
    let Some(mut reward) = diagnostics.reward.take() else {
        "NO ACTIVE REWARD".clone_into(&mut diagnostics.reward_result);
        return;
    };
    match death.apply_reward_choice(&mut reward, action.choice(), runtime.combat().tick()) {
        Ok(RewardOutcome::LeftReward { .. }) => {
            "LEAVE ACCEPTED | REWARD PRESERVED".clone_into(&mut diagnostics.reward_result);
            diagnostics.reward = Some(reward);
        }
        Ok(RewardOutcome::CapacityBlocked { .. }) => {
            "BLOCKED | REWARD PRESERVED / NO DESTRUCTION"
                .clone_into(&mut diagnostics.reward_result);
            diagnostics.reward = Some(reward);
        }
        Ok(RewardOutcome::Collected { pickup, dropped }) => {
            diagnostics.reward_result = format!(
                "ACTION ACCEPTED: {pickup:?} | DROPPED EXISTING {}",
                dropped.is_some()
            );
        }
        Err(error) => {
            diagnostics.reward_result = format!("REJECTED | {error}");
            diagnostics.reward = Some(reward);
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn spawn_inventory_overlay(mut commands: Commands, scenario: Res<EvidenceScenario>) {
    commands.spawn((
        Name::new("Prototype inventory and reward overlay"),
        InventoryOverlay,
        Text::new("INVENTORY"),
        TextFont::from_font_size(12.0),
        TextColor(Color::srgb_u8(231, 224, 203)),
        Node {
            position_type: PositionType::Absolute,
            right: px(14),
            bottom: px(24),
            width: px(410),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(10, 12, 16, 236)),
        BorderColor::all(Color::srgba_u8(199, 164, 86, 220)),
        if *scenario == EvidenceScenario::InventoryShowcase {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        },
    ));
}

#[allow(clippy::needless_pass_by_value)]
fn update_inventory_overlay(
    keyboard: Res<ButtonInput<KeyCode>>,
    scenario: Res<EvidenceScenario>,
    death: Res<LocalDeathRuntime>,
    diagnostics: Res<InventoryDiagnostics>,
    mut overlay: Single<(&mut Text, &mut Visibility), With<InventoryOverlay>>,
) {
    // This presentation-only query remains isolated from gameplay state.
    if keyboard.just_pressed(INVENTORY_TOGGLE) {
        *overlay.1 = match *overlay.1 {
            Visibility::Hidden => Visibility::Inherited,
            _ => Visibility::Hidden,
        };
    }
    if *scenario == EvidenceScenario::InventoryShowcase {
        *overlay.1 = Visibility::Inherited;
    }
    let inventory = death.inventory();
    let equipped = EquipmentSlot::ALL.map(|slot| {
        inventory
            .equipped_item(slot)
            .map_or("EMPTY", |item| short_item(item.content_id().as_str()))
    });
    let backpack = inventory
        .backpack()
        .iter()
        .enumerate()
        .map(|(index, stack)| format!("{}:{}", index + 1, stack.as_ref().map_or("--", stack_label)))
        .collect::<Vec<_>>()
        .join("  ");
    let tonics: u8 = inventory
        .belt()
        .slots()
        .iter()
        .copied()
        .map(sim_core::BeltSlot::tonic_count)
        .sum();
    let reward = diagnostics
        .reward
        .as_ref()
        .map_or("RESOLVED", |reward| stack_label(reward.stack()));
    overlay.0.0 = format!(
        "INVENTORY [I] | EXPLICIT ACTIONS / NO SILENT DESTRUCTION\nWEAPON {} | RELIC {} | ARMOR {} | CHARM {}\nBACKPACK 8: {}\nBELT 2: RED TONIC x{} | TAKE -> LOWEST EMPTY INDEX\n{}\nREWARD: {}\n[1] TAKE  [2] EQUIP  [3] DROP ARMOR + EQUIP  [4] LEAVE\n{}",
        equipped[0],
        equipped[1],
        equipped[2],
        equipped[3],
        backpack,
        tonics,
        diagnostics.last_result,
        reward,
        if diagnostics.reward_result.is_empty() {
            "REWARD ACTION READY"
        } else {
            &diagnostics.reward_result
        },
    );
}

fn stack_label(stack: &InventoryStack) -> &'static str {
    match stack {
        InventoryStack::Equipment(item) => short_item(item.content_id().as_str()),
        InventoryStack::RedTonic { .. } => "TONIC",
    }
}

fn short_item(content_id: &str) -> &'static str {
    match content_id {
        "item.prototype.weapon.pine_crossbow" => "PINE CROSSBOW",
        "item.prototype.relic.dented_scope" => "DENTED SCOPE",
        "item.prototype.armor.reedcloth_wraps" => "REEDCLOTH WRAPS",
        "item.prototype.armor.parish_leather" => "PARISH LEATHER",
        "item.prototype.charm.still_eye" => "STILL EYE",
        _ => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reward_action_maps_to_the_exact_core_choice() {
        assert_eq!(RewardAction::Take.choice(), RewardChoice::Take);
        assert_eq!(RewardAction::Equip.choice(), RewardChoice::Equip);
        assert_eq!(RewardAction::Leave.choice(), RewardChoice::LeaveReward);
        assert!(matches!(
            RewardAction::DropArmorThenEquip.choice(),
            RewardChoice::DropExisting {
                location: OwnedItemLocation::Equipped(EquipmentSlot::Armor),
                then: PlacementChoice::Equip,
                ..
            }
        ));
    }
}
