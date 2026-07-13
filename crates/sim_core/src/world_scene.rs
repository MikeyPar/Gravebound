use std::collections::{BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{TilePoint, TileRectangle};

/// Product role of an immutable world scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorldSceneKind {
    SafeHub,
    PrivateDanger,
}

/// Entity families that a scene can forbid at its authoritative creation boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SceneCreationKind {
    Hostile,
    Damage,
    Projectile,
    Pickup,
    Drop,
}

/// Immutable interaction timing compiled from authored content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionDefinition {
    pub range_milli_tiles: i32,
    pub hold_ticks: u16,
}

/// Dynamic authority condition applied after the independent integration gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SceneObjectCondition {
    Always,
    RequiresMicrorealmCleared,
}

/// Exact northwest-origin authored geometry for a scene child.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SceneObjectGeometry {
    Point(TilePoint),
    PointInteractable {
        point: TilePoint,
        clear_radius_milli_tiles: i32,
    },
    Circle {
        center: TilePoint,
        radius_milli_tiles: i32,
    },
    Rectangle(TileRectangle),
}

/// Stable child object consumed by both authority and presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldSceneObject {
    pub id: String,
    pub geometry: SceneObjectGeometry,
    pub interaction: Option<InteractionDefinition>,
    pub integration_gate: Option<String>,
    pub condition: SceneObjectCondition,
}

/// Axis-aligned authored road. Roads are presentation/navigation data and never implicit collision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldRoad {
    pub width_milli_tiles: i32,
    pub points: Vec<TilePoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneDisplacement {
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
}

impl SceneDisplacement {
    #[must_use]
    pub const fn new(x_milli_tiles: i32, y_milli_tiles: i32) -> Self {
        Self {
            x_milli_tiles,
            y_milli_tiles,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SceneAccessContext<'a> {
    pub enabled_integration_gates: &'a BTreeSet<String>,
    pub microrealm_cleared: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneInteractionAccess {
    Available,
    StageDisabled,
    ConditionUnmet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneInteractionProjection {
    pub object_id: String,
    pub hold_ticks: u16,
    pub access: SceneInteractionAccess,
    pub distance_squared_milli_tiles: i128,
}

/// Player-owned fixed-point navigation state for one immutable scene definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldScenePlayer {
    scene_id: String,
    position: TilePoint,
    maximum_step_milli_tiles: i32,
}

impl WorldScenePlayer {
    pub fn new(
        scene: &WorldSceneDefinition,
        position: TilePoint,
        maximum_step_milli_tiles: i32,
    ) -> Result<Self, WorldSceneError> {
        if maximum_step_milli_tiles <= 0 {
            return Err(WorldSceneError::InvalidMovementStep);
        }
        if !scene.can_occupy(position) {
            return Err(WorldSceneError::BlockedPlayerSpawn);
        }
        Ok(Self {
            scene_id: scene.id.clone(),
            position,
            maximum_step_milli_tiles,
        })
    }

    #[must_use]
    pub const fn position(&self) -> TilePoint {
        self.position
    }

    pub fn step_movement(
        &mut self,
        scene: &WorldSceneDefinition,
        displacement: SceneDisplacement,
    ) -> Result<TilePoint, WorldSceneError> {
        self.require_scene(scene)?;
        let dx = i64::from(displacement.x_milli_tiles);
        let dy = i64::from(displacement.y_milli_tiles);
        let maximum = i64::from(self.maximum_step_milli_tiles);
        if dx * dx + dy * dy > maximum * maximum {
            return Err(WorldSceneError::MovementStepExceeded);
        }
        let horizontal = TilePoint::new(
            self.position
                .x_milli_tiles
                .checked_add(displacement.x_milli_tiles)
                .ok_or(WorldSceneError::Overflow)?,
            self.position.y_milli_tiles,
        );
        if scene.can_occupy(horizontal) {
            self.position = horizontal;
        }
        let vertical = TilePoint::new(
            self.position.x_milli_tiles,
            self.position
                .y_milli_tiles
                .checked_add(displacement.y_milli_tiles)
                .ok_or(WorldSceneError::Overflow)?,
        );
        if scene.can_occupy(vertical) {
            self.position = vertical;
        }
        Ok(self.position)
    }

    pub fn nearest_interaction(
        &self,
        scene: &WorldSceneDefinition,
        context: SceneAccessContext<'_>,
    ) -> Result<Option<SceneInteractionProjection>, WorldSceneError> {
        self.require_scene(scene)?;
        Ok(scene
            .objects
            .iter()
            .filter_map(|object| {
                let interaction = object.interaction?;
                let SceneObjectGeometry::PointInteractable { point, .. } = object.geometry else {
                    return None;
                };
                let distance_squared = squared_distance(self.position, point);
                let range = i128::from(interaction.range_milli_tiles);
                (distance_squared <= range * range).then(|| SceneInteractionProjection {
                    object_id: object.id.clone(),
                    hold_ticks: interaction.hold_ticks,
                    access: object_access(object, context),
                    distance_squared_milli_tiles: distance_squared,
                })
            })
            .min_by(|left, right| {
                left.distance_squared_milli_tiles
                    .cmp(&right.distance_squared_milli_tiles)
                    .then_with(|| left.object_id.cmp(&right.object_id))
            }))
    }

    fn require_scene(&self, scene: &WorldSceneDefinition) -> Result<(), WorldSceneError> {
        if self.scene_id != scene.id {
            return Err(WorldSceneError::SceneMismatch);
        }
        Ok(())
    }
}

/// Renderer-independent immutable scene definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldSceneDefinition {
    pub id: String,
    pub kind: WorldSceneKind,
    pub width_milli_tiles: i32,
    pub height_milli_tiles: i32,
    pub shell_thickness_milli_tiles: i32,
    pub player_radius_milli_tiles: i32,
    pub capacity: Option<u16>,
    pub player_spawn: TilePoint,
    pub solid_rectangles: Vec<TileRectangle>,
    pub roads: Vec<WorldRoad>,
    pub objects: Vec<WorldSceneObject>,
    pub prohibited_creation: BTreeSet<SceneCreationKind>,
}

impl WorldSceneDefinition {
    /// Validates and returns one immutable scene.
    pub fn validated(self) -> Result<Self, WorldSceneError> {
        self.validate_invariants()?;
        Ok(self)
    }

    /// Returns whether a radius-aware player center can occupy the authored point.
    #[must_use]
    pub fn can_occupy(&self, point: TilePoint) -> bool {
        self.inside_walkable_shell(point)
            && !self.solid_rectangles.iter().copied().any(|solid| {
                circle_intersects_rectangle(point, self.player_radius_milli_tiles, solid)
            })
    }

    /// Returns a stable BLAKE3 digest of the validated authoritative definition.
    pub fn deterministic_digest(&self) -> Result<[u8; 32], WorldSceneError> {
        self.validate_invariants()?;
        let bytes = postcard::to_stdvec(self).map_err(|_| WorldSceneError::Serialization)?;
        Ok(*blake3::hash(&bytes).as_bytes())
    }

    #[must_use]
    pub fn allows_creation(&self, kind: SceneCreationKind) -> bool {
        !self.prohibited_creation.contains(&kind)
    }

    /// Proves grid-connected radius-aware navigation between two authored points.
    pub fn has_grid_path(
        &self,
        start: TilePoint,
        goal: TilePoint,
        grid_step_milli_tiles: i32,
    ) -> Result<bool, WorldSceneError> {
        if grid_step_milli_tiles <= 0 {
            return Err(WorldSceneError::InvalidNavigationStep);
        }
        if !self.can_occupy(start) || !self.can_occupy(goal) {
            return Ok(false);
        }
        let mut open = VecDeque::from([start]);
        let mut visited = BTreeSet::from([start]);
        while let Some(point) = open.pop_front() {
            if point == goal {
                return Ok(true);
            }
            for (dx, dy) in [
                (grid_step_milli_tiles, 0),
                (-grid_step_milli_tiles, 0),
                (0, grid_step_milli_tiles),
                (0, -grid_step_milli_tiles),
            ] {
                let (Some(x), Some(y)) = (
                    point.x_milli_tiles.checked_add(dx),
                    point.y_milli_tiles.checked_add(dy),
                ) else {
                    continue;
                };
                let neighbor = TilePoint::new(x, y);
                if self.can_occupy(neighbor) && visited.insert(neighbor) {
                    open.push_back(neighbor);
                }
            }
        }
        Ok(false)
    }

    fn validate_invariants(&self) -> Result<(), WorldSceneError> {
        if self.id.trim().is_empty() {
            return Err(WorldSceneError::EmptySceneId);
        }
        if self.width_milli_tiles <= 0 || self.height_milli_tiles <= 0 {
            return Err(WorldSceneError::InvalidBounds);
        }
        if self.shell_thickness_milli_tiles <= 0
            || self.player_radius_milli_tiles <= 0
            || self.capacity == Some(0)
        {
            return Err(WorldSceneError::InvalidPhysicalPolicy);
        }
        if self.kind == WorldSceneKind::PrivateDanger && self.capacity.is_none() {
            return Err(WorldSceneError::InvalidPhysicalPolicy);
        }
        let minimum_dimension = self
            .shell_thickness_milli_tiles
            .checked_add(self.player_radius_milli_tiles)
            .and_then(|margin| margin.checked_mul(2))
            .ok_or(WorldSceneError::Overflow)?;
        if minimum_dimension >= self.width_milli_tiles
            || minimum_dimension >= self.height_milli_tiles
        {
            return Err(WorldSceneError::InvalidPhysicalPolicy);
        }

        for (index, solid) in self.solid_rectangles.iter().copied().enumerate() {
            validate_rectangle(solid, self.width_milli_tiles, self.height_milli_tiles)
                .map_err(|()| WorldSceneError::InvalidSolid { index })?;
        }
        for first in 0..self.solid_rectangles.len() {
            for second in (first + 1)..self.solid_rectangles.len() {
                if self.solid_rectangles[first].overlaps(self.solid_rectangles[second]) {
                    return Err(WorldSceneError::OverlappingSolids { first, second });
                }
            }
        }
        if !self.can_occupy(self.player_spawn) {
            return Err(WorldSceneError::BlockedPlayerSpawn);
        }

        let mut object_ids = BTreeSet::new();
        for object in &self.objects {
            if object.id.trim().is_empty() {
                return Err(WorldSceneError::EmptyObjectId);
            }
            if !object_ids.insert(object.id.as_str()) {
                return Err(WorldSceneError::DuplicateObjectId(object.id.clone()));
            }
            self.validate_object_geometry(object)?;
            if object
                .integration_gate
                .as_ref()
                .is_some_and(|gate| gate.trim().is_empty())
            {
                return Err(WorldSceneError::InvalidIntegrationGate(object.id.clone()));
            }
            if let Some(interaction) = object.interaction
                && interaction.range_milli_tiles <= 0
            {
                return Err(WorldSceneError::InvalidInteraction(object.id.clone()));
            }
            if object.interaction.is_some()
                && !matches!(
                    object.geometry,
                    SceneObjectGeometry::PointInteractable { .. }
                )
            {
                return Err(WorldSceneError::InvalidInteraction(object.id.clone()));
            }
        }

        for (index, road) in self.roads.iter().enumerate() {
            if road.width_milli_tiles <= 0 || road.points.len() < 2 {
                return Err(WorldSceneError::InvalidRoad { index });
            }
            for point in &road.points {
                self.validate_point(*point)
                    .map_err(|_| WorldSceneError::InvalidRoad { index })?;
            }
            if road.points.windows(2).any(|pair| {
                pair[0].x_milli_tiles != pair[1].x_milli_tiles
                    && pair[0].y_milli_tiles != pair[1].y_milli_tiles
            }) {
                return Err(WorldSceneError::NonAxisAlignedRoad { index });
            }
        }
        Ok(())
    }

    fn validate_object_geometry(&self, object: &WorldSceneObject) -> Result<(), WorldSceneError> {
        match object.geometry {
            SceneObjectGeometry::Point(point) => self
                .validate_point(point)
                .map_err(|_| WorldSceneError::InvalidObjectGeometry(object.id.clone())),
            SceneObjectGeometry::PointInteractable {
                point,
                clear_radius_milli_tiles,
            } => {
                if clear_radius_milli_tiles <= 0 {
                    return Err(WorldSceneError::InvalidObjectGeometry(object.id.clone()));
                }
                self.validate_point(point)
                    .map_err(|_| WorldSceneError::InvalidObjectGeometry(object.id.clone()))
            }
            SceneObjectGeometry::Circle {
                center,
                radius_milli_tiles,
            } => {
                if radius_milli_tiles <= 0 {
                    return Err(WorldSceneError::InvalidObjectGeometry(object.id.clone()));
                }
                let left = center
                    .x_milli_tiles
                    .checked_sub(radius_milli_tiles)
                    .ok_or(WorldSceneError::Overflow)?;
                let top = center
                    .y_milli_tiles
                    .checked_sub(radius_milli_tiles)
                    .ok_or(WorldSceneError::Overflow)?;
                let right = center
                    .x_milli_tiles
                    .checked_add(radius_milli_tiles)
                    .ok_or(WorldSceneError::Overflow)?;
                let bottom = center
                    .y_milli_tiles
                    .checked_add(radius_milli_tiles)
                    .ok_or(WorldSceneError::Overflow)?;
                if left < 0
                    || top < 0
                    || right > self.width_milli_tiles
                    || bottom > self.height_milli_tiles
                {
                    return Err(WorldSceneError::InvalidObjectGeometry(object.id.clone()));
                }
                Ok(())
            }
            SceneObjectGeometry::Rectangle(rectangle) => {
                validate_rectangle(rectangle, self.width_milli_tiles, self.height_milli_tiles)
                    .map_err(|()| WorldSceneError::InvalidObjectGeometry(object.id.clone()))
            }
        }
    }

    fn validate_point(&self, point: TilePoint) -> Result<(), WorldSceneError> {
        if point.x_milli_tiles < 0
            || point.y_milli_tiles < 0
            || point.x_milli_tiles >= self.width_milli_tiles
            || point.y_milli_tiles >= self.height_milli_tiles
        {
            return Err(WorldSceneError::PointOutOfBounds);
        }
        Ok(())
    }

    fn inside_walkable_shell(&self, point: TilePoint) -> bool {
        let Some(minimum) = self
            .shell_thickness_milli_tiles
            .checked_add(self.player_radius_milli_tiles)
        else {
            return false;
        };
        let Some(maximum_x) = self.width_milli_tiles.checked_sub(minimum) else {
            return false;
        };
        let Some(maximum_y) = self.height_milli_tiles.checked_sub(minimum) else {
            return false;
        };
        point.x_milli_tiles >= minimum
            && point.y_milli_tiles >= minimum
            && point.x_milli_tiles <= maximum_x
            && point.y_milli_tiles <= maximum_y
    }
}

fn object_access(
    object: &WorldSceneObject,
    context: SceneAccessContext<'_>,
) -> SceneInteractionAccess {
    if object
        .integration_gate
        .as_ref()
        .is_some_and(|gate| !context.enabled_integration_gates.contains(gate))
    {
        SceneInteractionAccess::StageDisabled
    } else if object.condition == SceneObjectCondition::RequiresMicrorealmCleared
        && !context.microrealm_cleared
    {
        SceneInteractionAccess::ConditionUnmet
    } else {
        SceneInteractionAccess::Available
    }
}

fn squared_distance(left: TilePoint, right: TilePoint) -> i128 {
    let dx = i128::from(left.x_milli_tiles) - i128::from(right.x_milli_tiles);
    let dy = i128::from(left.y_milli_tiles) - i128::from(right.y_milli_tiles);
    dx * dx + dy * dy
}

fn validate_rectangle(
    rectangle: TileRectangle,
    width_milli_tiles: i32,
    height_milli_tiles: i32,
) -> Result<(), ()> {
    if rectangle.width_milli_tiles <= 0 || rectangle.height_milli_tiles <= 0 {
        return Err(());
    }
    let right = rectangle.right().ok_or(())?;
    let bottom = rectangle.bottom().ok_or(())?;
    if rectangle.x_milli_tiles < 0
        || rectangle.y_milli_tiles < 0
        || right > width_milli_tiles
        || bottom > height_milli_tiles
    {
        return Err(());
    }
    Ok(())
}

fn circle_intersects_rectangle(
    center: TilePoint,
    radius_milli_tiles: i32,
    rectangle: TileRectangle,
) -> bool {
    let Some(right) = rectangle.right() else {
        return true;
    };
    let Some(bottom) = rectangle.bottom() else {
        return true;
    };
    let closest_x = center.x_milli_tiles.clamp(rectangle.x_milli_tiles, right);
    let closest_y = center.y_milli_tiles.clamp(rectangle.y_milli_tiles, bottom);
    let dx = i64::from(center.x_milli_tiles) - i64::from(closest_x);
    let dy = i64::from(center.y_milli_tiles) - i64::from(closest_y);
    let radius = i64::from(radius_milli_tiles);
    dx * dx + dy * dy <= radius * radius
}

/// Fail-closed scene-definition error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldSceneError {
    #[error("scene ID must not be empty")]
    EmptySceneId,
    #[error("scene width and height must be positive")]
    InvalidBounds,
    #[error("scene shell, player radius, and capacity must define a walkable interior")]
    InvalidPhysicalPolicy,
    #[error("solid {index} is invalid")]
    InvalidSolid { index: usize },
    #[error("solids {first} and {second} overlap")]
    OverlappingSolids { first: usize, second: usize },
    #[error("player spawn is outside the radius-aware walkable area")]
    BlockedPlayerSpawn,
    #[error("scene object ID must not be empty")]
    EmptyObjectId,
    #[error("duplicate scene object ID `{0}`")]
    DuplicateObjectId(String),
    #[error("scene object `{0}` has invalid geometry")]
    InvalidObjectGeometry(String),
    #[error("scene object `{0}` has invalid interaction policy")]
    InvalidInteraction(String),
    #[error("scene object `{0}` has an empty integration gate")]
    InvalidIntegrationGate(String),
    #[error("scene player maximum step must be positive")]
    InvalidMovementStep,
    #[error("scene player displacement exceeded its authoritative maximum")]
    MovementStepExceeded,
    #[error("scene player was used with a different scene definition")]
    SceneMismatch,
    #[error("navigation grid step must be positive")]
    InvalidNavigationStep,
    #[error("road {index} is invalid")]
    InvalidRoad { index: usize },
    #[error("road {index} contains a diagonal segment")]
    NonAxisAlignedRoad { index: usize },
    #[error("scene point is outside authored bounds")]
    PointOutOfBounds,
    #[error("scene geometry arithmetic overflow")]
    Overflow,
    #[error("scene definition could not be serialized deterministically")]
    Serialization,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> WorldSceneDefinition {
        WorldSceneDefinition {
            id: "hub.test".to_owned(),
            kind: WorldSceneKind::SafeHub,
            width_milli_tiles: 20_000,
            height_milli_tiles: 16_000,
            shell_thickness_milli_tiles: 1_000,
            player_radius_milli_tiles: 300,
            capacity: None,
            player_spawn: TilePoint::new(10_000, 13_000),
            solid_rectangles: vec![TileRectangle::new(8_000, 6_000, 4_000, 2_000)],
            roads: vec![WorldRoad {
                width_milli_tiles: 3_000,
                points: vec![
                    TilePoint::new(3_000, 13_000),
                    TilePoint::new(17_000, 13_000),
                ],
            }],
            objects: vec![WorldSceneObject {
                id: "station.test".to_owned(),
                geometry: SceneObjectGeometry::PointInteractable {
                    point: TilePoint::new(10_000, 3_000),
                    clear_radius_milli_tiles: 2_000,
                },
                interaction: Some(InteractionDefinition {
                    range_milli_tiles: 1_500,
                    hold_ticks: 15,
                }),
                integration_gate: Some("core_world_flow_integration".to_owned()),
                condition: SceneObjectCondition::Always,
            }],
            prohibited_creation: BTreeSet::from([
                SceneCreationKind::Hostile,
                SceneCreationKind::Damage,
                SceneCreationKind::Projectile,
                SceneCreationKind::Pickup,
                SceneCreationKind::Drop,
            ]),
        }
        .validated()
        .expect("valid sample")
    }

    #[test]
    fn radius_aware_collision_rejects_shell_and_interior_solids() {
        let scene = sample();
        assert!(scene.can_occupy(TilePoint::new(10_000, 13_000)));
        assert!(!scene.can_occupy(TilePoint::new(1_200, 5_000)));
        assert!(!scene.can_occupy(TilePoint::new(7_800, 7_000)));
        assert!(scene.can_occupy(TilePoint::new(7_699, 7_000)));
    }

    #[test]
    fn deterministic_digest_changes_with_authoritative_geometry() {
        let scene = sample();
        let digest = scene.deterministic_digest().expect("digest");
        assert_eq!(digest, scene.deterministic_digest().expect("stable digest"));

        let mut changed = scene.clone();
        changed.player_spawn.x_milli_tiles += 1;
        assert_ne!(
            digest,
            changed.deterministic_digest().expect("changed digest")
        );
    }

    #[test]
    fn invalid_geometry_fails_closed() {
        let mut blocked = sample();
        blocked.player_spawn = TilePoint::new(9_000, 7_000);
        assert_eq!(
            blocked.validated(),
            Err(WorldSceneError::BlockedPlayerSpawn)
        );

        let mut diagonal = sample();
        diagonal.roads[0].points[1] = TilePoint::new(17_000, 12_000);
        assert_eq!(
            diagonal.validated(),
            Err(WorldSceneError::NonAxisAlignedRoad { index: 0 })
        );
    }

    #[test]
    fn fixed_point_movement_slides_without_crossing_collision() {
        let scene = sample();
        let mut player =
            WorldScenePlayer::new(&scene, TilePoint::new(7_600, 8_200), 250).expect("player");
        assert_eq!(
            player
                .step_movement(&scene, SceneDisplacement::new(200, -100))
                .expect("bounded movement"),
            TilePoint::new(7_600, 8_100)
        );
        assert_eq!(
            player.step_movement(&scene, SceneDisplacement::new(251, 0)),
            Err(WorldSceneError::MovementStepExceeded)
        );
    }

    #[test]
    fn interaction_projection_is_range_ordered_and_authority_gated() {
        let scene = sample();
        let player =
            WorldScenePlayer::new(&scene, TilePoint::new(10_000, 4_400), 200).expect("player");
        let disabled = player
            .nearest_interaction(
                &scene,
                SceneAccessContext {
                    enabled_integration_gates: &BTreeSet::new(),
                    microrealm_cleared: false,
                },
            )
            .expect("projection")
            .expect("near station");
        assert_eq!(disabled.object_id, "station.test");
        assert_eq!(disabled.access, SceneInteractionAccess::StageDisabled);
        assert_eq!(disabled.hold_ticks, 15);

        let enabled = BTreeSet::from(["core_world_flow_integration".to_owned()]);
        assert_eq!(
            player
                .nearest_interaction(
                    &scene,
                    SceneAccessContext {
                        enabled_integration_gates: &enabled,
                        microrealm_cleared: false,
                    },
                )
                .expect("projection")
                .expect("near station")
                .access,
            SceneInteractionAccess::Available
        );
    }

    #[test]
    fn navigation_and_safe_creation_policy_are_explicit() {
        let scene = sample();
        assert!(
            scene
                .has_grid_path(
                    TilePoint::new(10_000, 13_000),
                    TilePoint::new(10_000, 3_000),
                    500,
                )
                .expect("path query")
        );
        for prohibited in [
            SceneCreationKind::Hostile,
            SceneCreationKind::Damage,
            SceneCreationKind::Projectile,
            SceneCreationKind::Pickup,
            SceneCreationKind::Drop,
        ] {
            assert!(!scene.allows_creation(prohibited));
        }
        assert_eq!(
            scene.has_grid_path(
                TilePoint::new(10_000, 13_000),
                TilePoint::new(10_000, 3_000),
                0,
            ),
            Err(WorldSceneError::InvalidNavigationStep)
        );
    }
}
