//! Renderer-independent authored dungeon rooms, rotation, and fixed-layout placement.

use std::collections::{BTreeSet, VecDeque};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DungeonDoorSide {
    North,
    East,
    South,
    West,
}

impl DungeonDoorSide {
    #[must_use]
    pub const fn rotated_clockwise(self, quarter_turns: u8) -> Self {
        let index = match self {
            Self::North => 0,
            Self::East => 1,
            Self::South => 2,
            Self::West => 3,
        };
        match (index + quarter_turns % 4) % 4 {
            0 => Self::North,
            1 => Self::East,
            2 => Self::South,
            _ => Self::West,
        }
    }

    #[must_use]
    pub const fn opposite(self) -> Self {
        self.rotated_clockwise(2)
    }

    #[must_use]
    pub const fn outward_unit(self) -> (i32, i32) {
        match self {
            Self::North => (0, -1),
            Self::East => (1, 0),
            Self::South => (0, 1),
            Self::West => (-1, 0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DungeonDoorDefinition {
    pub id: String,
    pub side: DungeonDoorSide,
    pub offset_milli_tiles: u32,
    pub width_milli_tiles: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DungeonRoomVolumeKind {
    Solid,
    DeepWater,
    WalkableBoundary,
    PatternLane,
    ObjectiveArea,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DungeonRoomVolumeGeometry {
    Rectangle {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    Circle {
        x: i32,
        y: i32,
        radius: u32,
    },
    Polyline {
        width_milli_tiles: u32,
        points: Vec<(i32, i32)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DungeonRoomVolume {
    pub id: String,
    pub kind: DungeonRoomVolumeKind,
    pub geometry: DungeonRoomVolumeGeometry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DungeonAnchorKind {
    SafeEntry,
    Exit,
    Fodder,
    Pressure,
    Disruptor,
    AnchorEnemy,
    Miniboss,
    Stage,
    Add,
    Shrine,
    Stabilization,
    Chest,
    Boss,
    ChargeEndpoint,
    Group,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DungeonRoomAnchor {
    pub id: String,
    pub kind: DungeonAnchorKind,
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
    pub bound_content_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DungeonRoomDefinition {
    pub id: String,
    pub width_milli_tiles: u32,
    pub height_milli_tiles: u32,
    pub doors: Vec<DungeonDoorDefinition>,
    pub volumes: Vec<DungeonRoomVolume>,
    pub anchors: Vec<DungeonRoomAnchor>,
    pub safe_noncombat: bool,
}

impl DungeonRoomDefinition {
    pub fn validated(self) -> Result<Self, DungeonRoomError> {
        if self.id.is_empty()
            || self.width_milli_tiles == 0
            || self.height_milli_tiles == 0
            || self.width_milli_tiles > u32::try_from(i32::MAX).expect("i32 max fits u32")
            || self.height_milli_tiles > u32::try_from(i32::MAX).expect("i32 max fits u32")
        {
            return Err(DungeonRoomError::InvalidRoomIdentityOrSize);
        }
        unique(self.doors.iter().map(|door| door.id.as_str()), "door")?;
        unique(
            self.volumes.iter().map(|volume| volume.id.as_str()),
            "volume",
        )?;
        unique(
            self.anchors.iter().map(|anchor| anchor.id.as_str()),
            "anchor",
        )?;
        for door in &self.doors {
            let edge = match door.side {
                DungeonDoorSide::North | DungeonDoorSide::South => self.width_milli_tiles,
                DungeonDoorSide::East | DungeonDoorSide::West => self.height_milli_tiles,
            };
            if door.id.is_empty() || door.width_milli_tiles == 0 || door.offset_milli_tiles > edge {
                return Err(DungeonRoomError::InvalidDoor(door.id.clone()));
            }
        }
        for anchor in &self.anchors {
            if anchor.id.is_empty()
                || !point_in_bounds(
                    anchor.x_milli_tiles,
                    anchor.y_milli_tiles,
                    self.width_milli_tiles,
                    self.height_milli_tiles,
                )
            {
                return Err(DungeonRoomError::InvalidAnchor(anchor.id.clone()));
            }
        }
        for volume in &self.volumes {
            if volume.id.is_empty()
                || !geometry_in_bounds(
                    &volume.geometry,
                    self.width_milli_tiles,
                    self.height_milli_tiles,
                )
            {
                return Err(DungeonRoomError::InvalidVolume(volume.id.clone()));
            }
        }
        Ok(self)
    }

    pub fn rotated(&self, rotation_degrees: u16) -> Result<RotatedDungeonRoom, DungeonRoomError> {
        let quarter_turns = rotation_quarter_turns(rotation_degrees)?;
        let (width, height) = rotated_dimensions(
            self.width_milli_tiles,
            self.height_milli_tiles,
            quarter_turns,
        );
        let doors = self
            .doors
            .iter()
            .map(|door| {
                let (x, y) = door_point(door, self.width_milli_tiles, self.height_milli_tiles);
                let (x, y) = rotate_point(
                    x,
                    y,
                    self.width_milli_tiles,
                    self.height_milli_tiles,
                    quarter_turns,
                );
                RotatedDungeonDoor {
                    id: door.id.clone(),
                    side: door.side.rotated_clockwise(quarter_turns),
                    x_milli_tiles: x,
                    y_milli_tiles: y,
                    width_milli_tiles: door.width_milli_tiles,
                }
            })
            .collect();
        let volumes = self
            .volumes
            .iter()
            .map(|volume| DungeonRoomVolume {
                id: volume.id.clone(),
                kind: volume.kind,
                geometry: rotate_geometry(
                    &volume.geometry,
                    self.width_milli_tiles,
                    self.height_milli_tiles,
                    quarter_turns,
                ),
            })
            .collect();
        let anchors = self
            .anchors
            .iter()
            .map(|anchor| {
                let (x, y) = rotate_point(
                    anchor.x_milli_tiles,
                    anchor.y_milli_tiles,
                    self.width_milli_tiles,
                    self.height_milli_tiles,
                    quarter_turns,
                );
                DungeonRoomAnchor {
                    id: anchor.id.clone(),
                    kind: anchor.kind,
                    x_milli_tiles: x,
                    y_milli_tiles: y,
                    bound_content_id: anchor.bound_content_id.clone(),
                }
            })
            .collect();
        Ok(RotatedDungeonRoom {
            room_id: self.id.clone(),
            rotation_degrees,
            width_milli_tiles: width,
            height_milli_tiles: height,
            doors,
            volumes,
            anchors,
            safe_noncombat: self.safe_noncombat,
        })
    }

    /// Proves door-to-door and selected utility-anchor reachability on a deterministic fixed grid.
    pub fn prove_navigation(
        &self,
        required_anchor_kinds: &[DungeonAnchorKind],
        actor_radius_milli_tiles: u32,
        grid_step_milli_tiles: u32,
    ) -> Result<DungeonRoomNavigationEvidence, DungeonRoomError> {
        if actor_radius_milli_tiles == 0
            || grid_step_milli_tiles == 0
            || grid_step_milli_tiles > i32::MAX.unsigned_abs()
        {
            return Err(DungeonRoomError::InvalidNavigationParameters);
        }
        let inset = i32::try_from(grid_step_milli_tiles)
            .map_err(|_| DungeonRoomError::InvalidNavigationParameters)?;
        let mut targets = Vec::with_capacity(self.doors.len() + self.anchors.len());
        for door in &self.doors {
            let (x, y) = door_point(door, self.width_milli_tiles, self.height_milli_tiles);
            let (outward_x, outward_y) = door.side.outward_unit();
            let point = (1..=16)
                .map(|steps| (x - outward_x * inset * steps, y - outward_y * inset * steps))
                .find(|point| self.navigation_point_is_walkable(*point, actor_radius_milli_tiles))
                .ok_or_else(|| DungeonRoomError::InvalidDoor(door.id.clone()))?;
            targets.push(point);
        }
        let required_kinds = required_anchor_kinds
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        targets.extend(
            self.anchors
                .iter()
                .filter(|anchor| required_kinds.contains(&anchor.kind))
                .map(|anchor| (anchor.x_milli_tiles, anchor.y_milli_tiles)),
        );
        if targets.is_empty() {
            return Err(DungeonRoomError::MissingNavigationTarget);
        }
        for point in &targets {
            if !self.navigation_point_is_walkable(*point, actor_radius_milli_tiles) {
                return Err(DungeonRoomError::UnwalkableNavigationPoint {
                    x: point.0,
                    y: point.1,
                });
            }
        }
        let visited = self.navigation_flood(targets[0], actor_radius_milli_tiles, inset);
        if targets.iter().any(|target| !visited.contains(target)) {
            return Err(DungeonRoomError::DisconnectedNavigation);
        }
        Ok(DungeonRoomNavigationEvidence {
            visited_grid_points: visited.len(),
            required_target_count: targets.len(),
        })
    }

    fn navigation_flood(
        &self,
        start: (i32, i32),
        actor_radius_milli_tiles: u32,
        step: i32,
    ) -> BTreeSet<(i32, i32)> {
        let mut visited = BTreeSet::from([start]);
        let mut pending = VecDeque::from([start]);
        while let Some((x, y)) = pending.pop_front() {
            for (dx, dy) in [(0, -step), (step, 0), (0, step), (-step, 0)] {
                let (Some(next_x), Some(next_y)) = (x.checked_add(dx), y.checked_add(dy)) else {
                    continue;
                };
                let next = (next_x, next_y);
                if !visited.contains(&next)
                    && self.navigation_point_is_walkable(next, actor_radius_milli_tiles)
                {
                    visited.insert(next);
                    pending.push_back(next);
                }
            }
        }
        visited
    }

    fn navigation_point_is_walkable(
        &self,
        (x, y): (i32, i32),
        actor_radius_milli_tiles: u32,
    ) -> bool {
        let radius = i64::from(actor_radius_milli_tiles);
        let x = i64::from(x);
        let y = i64::from(y);
        if x < radius
            || y < radius
            || x > i64::from(self.width_milli_tiles) - radius
            || y > i64::from(self.height_milli_tiles) - radius
        {
            return false;
        }
        for volume in &self.volumes {
            match (&volume.kind, &volume.geometry) {
                (
                    DungeonRoomVolumeKind::Solid | DungeonRoomVolumeKind::DeepWater,
                    DungeonRoomVolumeGeometry::Rectangle {
                        x: left,
                        y: top,
                        width,
                        height,
                    },
                ) => {
                    let rectangle_left = i64::from(*left);
                    let rectangle_top = i64::from(*top);
                    let expanded_left = rectangle_left - radius;
                    let expanded_top = rectangle_top - radius;
                    let expanded_right = rectangle_left + i64::from(*width) + radius;
                    let expanded_bottom = rectangle_top + i64::from(*height) + radius;
                    if x > expanded_left
                        && x < expanded_right
                        && y > expanded_top
                        && y < expanded_bottom
                    {
                        return false;
                    }
                }
                (
                    DungeonRoomVolumeKind::WalkableBoundary,
                    DungeonRoomVolumeGeometry::Circle {
                        x: center_x,
                        y: center_y,
                        radius: boundary_radius,
                    },
                ) => {
                    let usable_radius = i64::from(*boundary_radius) - radius;
                    let dx = x - i64::from(*center_x);
                    let dy = y - i64::from(*center_y);
                    if usable_radius <= 0
                        || dx.saturating_mul(dx) + dy.saturating_mul(dy)
                            > usable_radius.saturating_mul(usable_radius)
                    {
                        return false;
                    }
                }
                _ => {}
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DungeonRoomNavigationEvidence {
    pub visited_grid_points: usize,
    pub required_target_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotatedDungeonDoor {
    pub id: String,
    pub side: DungeonDoorSide,
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
    pub width_milli_tiles: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotatedDungeonRoom {
    pub room_id: String,
    pub rotation_degrees: u16,
    pub width_milli_tiles: u32,
    pub height_milli_tiles: u32,
    pub doors: Vec<RotatedDungeonDoor>,
    pub volumes: Vec<DungeonRoomVolume>,
    pub anchors: Vec<DungeonRoomAnchor>,
    pub safe_noncombat: bool,
}

impl RotatedDungeonRoom {
    #[must_use]
    pub fn door(&self, id: &str) -> Option<&RotatedDungeonDoor> {
        self.doors.iter().find(|door| door.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedDungeonRoom {
    pub node_id: String,
    pub room: RotatedDungeonRoom,
    pub origin_x_milli_tiles: i32,
    pub origin_y_milli_tiles: i32,
    pub counts_toward_room_total: bool,
}

impl PlacedDungeonRoom {
    pub fn world_door(&self, id: &str) -> Result<WorldDungeonDoor, DungeonRoomError> {
        let door = self
            .room
            .door(id)
            .ok_or_else(|| DungeonRoomError::UnknownDoor {
                node: self.node_id.clone(),
                door: id.to_owned(),
            })?;
        Ok(WorldDungeonDoor {
            side: door.side,
            x_milli_tiles: self
                .origin_x_milli_tiles
                .checked_add(door.x_milli_tiles)
                .ok_or(DungeonRoomError::CoordinateOverflow)?,
            y_milli_tiles: self
                .origin_y_milli_tiles
                .checked_add(door.y_milli_tiles)
                .ok_or(DungeonRoomError::CoordinateOverflow)?,
            width_milli_tiles: door.width_milli_tiles,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldDungeonDoor {
    pub side: DungeonDoorSide,
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
    pub width_milli_tiles: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DungeonCorridor {
    pub from_node_id: String,
    pub to_node_id: String,
    pub start_x_milli_tiles: i32,
    pub start_y_milli_tiles: i32,
    pub end_x_milli_tiles: i32,
    pub end_y_milli_tiles: i32,
    pub width_milli_tiles: u32,
    pub length_milli_tiles: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedDungeonLayoutDefinition {
    pub id: String,
    pub rooms: Vec<PlacedDungeonRoom>,
    pub corridors: Vec<DungeonCorridor>,
    pub disabled_branch_node_ids: Vec<String>,
}

impl FixedDungeonLayoutDefinition {
    pub fn validated(self) -> Result<Self, DungeonRoomError> {
        if self.id.is_empty() || self.rooms.is_empty() {
            return Err(DungeonRoomError::InvalidLayout);
        }
        unique(
            self.rooms.iter().map(|room| room.node_id.as_str()),
            "placed room",
        )?;
        unique(
            self.disabled_branch_node_ids.iter().map(String::as_str),
            "disabled branch",
        )?;
        let node_ids = self
            .rooms
            .iter()
            .map(|room| room.node_id.as_str())
            .collect::<BTreeSet<_>>();
        for corridor in &self.corridors {
            if !node_ids.contains(corridor.from_node_id.as_str())
                || !node_ids.contains(corridor.to_node_id.as_str())
                || corridor.width_milli_tiles == 0
                || corridor.length_milli_tiles == 0
            {
                return Err(DungeonRoomError::InvalidCorridor);
            }
            let dx =
                i64::from(corridor.end_x_milli_tiles) - i64::from(corridor.start_x_milli_tiles);
            let dy =
                i64::from(corridor.end_y_milli_tiles) - i64::from(corridor.start_y_milli_tiles);
            if (dx != 0 && dy != 0)
                || dx.unsigned_abs().saturating_add(dy.unsigned_abs())
                    != u64::from(corridor.length_milli_tiles)
            {
                return Err(DungeonRoomError::InvalidCorridor);
            }
        }
        Ok(self)
    }

    #[must_use]
    pub fn deterministic_digest(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        update_text(&mut hasher, &self.id);
        for room in &self.rooms {
            update_text(&mut hasher, &room.node_id);
            update_text(&mut hasher, &room.room.room_id);
            hasher.update(&room.room.rotation_degrees.to_le_bytes());
            hasher.update(&room.origin_x_milli_tiles.to_le_bytes());
            hasher.update(&room.origin_y_milli_tiles.to_le_bytes());
            hasher.update(&[u8::from(room.counts_toward_room_total)]);
        }
        for corridor in &self.corridors {
            update_text(&mut hasher, &corridor.from_node_id);
            update_text(&mut hasher, &corridor.to_node_id);
            hasher.update(&corridor.start_x_milli_tiles.to_le_bytes());
            hasher.update(&corridor.start_y_milli_tiles.to_le_bytes());
            hasher.update(&corridor.end_x_milli_tiles.to_le_bytes());
            hasher.update(&corridor.end_y_milli_tiles.to_le_bytes());
            hasher.update(&corridor.width_milli_tiles.to_le_bytes());
            hasher.update(&corridor.length_milli_tiles.to_le_bytes());
        }
        for node in &self.disabled_branch_node_ids {
            update_text(&mut hasher, node);
        }
        hasher.finalize().to_hex().to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DungeonRoomError {
    #[error("room identity or dimensions are invalid")]
    InvalidRoomIdentityOrSize,
    #[error("duplicate {0} identifier")]
    DuplicateId(String),
    #[error("invalid room door {0}")]
    InvalidDoor(String),
    #[error("invalid room volume {0}")]
    InvalidVolume(String),
    #[error("invalid room anchor {0}")]
    InvalidAnchor(String),
    #[error("rotation must be one of 0, 90, 180, or 270 degrees")]
    InvalidRotation,
    #[error("unknown door {door} on node {node}")]
    UnknownDoor { node: String, door: String },
    #[error("fixed dungeon layout is invalid")]
    InvalidLayout,
    #[error("fixed dungeon corridor is invalid")]
    InvalidCorridor,
    #[error("dungeon coordinate overflow")]
    CoordinateOverflow,
    #[error("dungeon navigation parameters are invalid")]
    InvalidNavigationParameters,
    #[error("dungeon room has no door or required utility anchor to prove")]
    MissingNavigationTarget,
    #[error("dungeon navigation point ({x},{y}) is not walkable")]
    UnwalkableNavigationPoint { x: i32, y: i32 },
    #[error("dungeon door and utility navigation targets are disconnected")]
    DisconnectedNavigation,
}

fn rotation_quarter_turns(rotation_degrees: u16) -> Result<u8, DungeonRoomError> {
    match rotation_degrees {
        0 => Ok(0),
        90 => Ok(1),
        180 => Ok(2),
        270 => Ok(3),
        _ => Err(DungeonRoomError::InvalidRotation),
    }
}

const fn rotated_dimensions(width: u32, height: u32, quarter_turns: u8) -> (u32, u32) {
    if quarter_turns.is_multiple_of(2) {
        (width, height)
    } else {
        (height, width)
    }
}

fn door_point(door: &DungeonDoorDefinition, width: u32, height: u32) -> (i32, i32) {
    let offset = i32::try_from(door.offset_milli_tiles).unwrap_or(i32::MAX);
    match door.side {
        DungeonDoorSide::North => (offset, 0),
        DungeonDoorSide::East => (i32::try_from(width).unwrap_or(i32::MAX), offset),
        DungeonDoorSide::South => (offset, i32::try_from(height).unwrap_or(i32::MAX)),
        DungeonDoorSide::West => (0, offset),
    }
}

fn rotate_point(x: i32, y: i32, width: u32, height: u32, quarter_turns: u8) -> (i32, i32) {
    let width = i32::try_from(width).unwrap_or(i32::MAX);
    let height = i32::try_from(height).unwrap_or(i32::MAX);
    match quarter_turns {
        0 => (x, y),
        1 => (height - y, x),
        2 => (width - x, height - y),
        _ => (y, width - x),
    }
}

fn rotate_geometry(
    geometry: &DungeonRoomVolumeGeometry,
    width: u32,
    height: u32,
    quarter_turns: u8,
) -> DungeonRoomVolumeGeometry {
    match geometry {
        DungeonRoomVolumeGeometry::Rectangle {
            x,
            y,
            width: rectangle_width,
            height: rectangle_height,
        } => {
            let corners = [
                rotate_point(*x, *y, width, height, quarter_turns),
                rotate_point(
                    *x + i32::try_from(*rectangle_width).unwrap_or(i32::MAX),
                    *y + i32::try_from(*rectangle_height).unwrap_or(i32::MAX),
                    width,
                    height,
                    quarter_turns,
                ),
            ];
            let min_x = corners[0].0.min(corners[1].0);
            let min_y = corners[0].1.min(corners[1].1);
            DungeonRoomVolumeGeometry::Rectangle {
                x: min_x,
                y: min_y,
                width: corners[0].0.abs_diff(corners[1].0),
                height: corners[0].1.abs_diff(corners[1].1),
            }
        }
        DungeonRoomVolumeGeometry::Circle { x, y, radius } => {
            let (x, y) = rotate_point(*x, *y, width, height, quarter_turns);
            DungeonRoomVolumeGeometry::Circle {
                x,
                y,
                radius: *radius,
            }
        }
        DungeonRoomVolumeGeometry::Polyline {
            width_milli_tiles,
            points,
        } => DungeonRoomVolumeGeometry::Polyline {
            width_milli_tiles: *width_milli_tiles,
            points: points
                .iter()
                .map(|(x, y)| rotate_point(*x, *y, width, height, quarter_turns))
                .collect(),
        },
    }
}

fn geometry_in_bounds(geometry: &DungeonRoomVolumeGeometry, width: u32, height: u32) -> bool {
    match geometry {
        DungeonRoomVolumeGeometry::Rectangle {
            x,
            y,
            width: rectangle_width,
            height: rectangle_height,
        } => {
            *rectangle_width > 0
                && *rectangle_height > 0
                && *x >= 0
                && *y >= 0
                && i64::from(*x) + i64::from(*rectangle_width) <= i64::from(width)
                && i64::from(*y) + i64::from(*rectangle_height) <= i64::from(height)
        }
        DungeonRoomVolumeGeometry::Circle { x, y, radius } => {
            *radius > 0 && point_in_bounds(*x, *y, width, height)
        }
        DungeonRoomVolumeGeometry::Polyline {
            width_milli_tiles,
            points,
        } => {
            *width_milli_tiles > 0
                && points.len() >= 2
                && points
                    .iter()
                    .all(|(x, y)| point_in_bounds(*x, *y, width, height))
        }
    }
}

fn point_in_bounds(x: i32, y: i32, width: u32, height: u32) -> bool {
    x >= 0 && y >= 0 && i64::from(x) <= i64::from(width) && i64::from(y) <= i64::from(height)
}

fn unique<'a>(
    values: impl IntoIterator<Item = &'a str>,
    domain: &str,
) -> Result<(), DungeonRoomError> {
    let values = values.into_iter().collect::<Vec<_>>();
    if values.iter().copied().collect::<BTreeSet<_>>().len() != values.len() {
        return Err(DungeonRoomError::DuplicateId(domain.to_owned()));
    }
    Ok(())
}

fn update_text(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(
        &u64::try_from(value.len())
            .expect("usize fits u64 on supported targets")
            .to_le_bytes(),
    );
    hasher.update(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> DungeonRoomDefinition {
        DungeonRoomDefinition {
            id: "room.test".to_owned(),
            width_milli_tiles: 13_000,
            height_milli_tiles: 11_000,
            doors: vec![DungeonDoorDefinition {
                id: "west".to_owned(),
                side: DungeonDoorSide::West,
                offset_milli_tiles: 5_500,
                width_milli_tiles: 3_000,
            }],
            volumes: vec![DungeonRoomVolume {
                id: "solid".to_owned(),
                kind: DungeonRoomVolumeKind::Solid,
                geometry: DungeonRoomVolumeGeometry::Rectangle {
                    x: 1_000,
                    y: 2_000,
                    width: 3_000,
                    height: 2_000,
                },
            }],
            anchors: vec![DungeonRoomAnchor {
                id: "spawn".to_owned(),
                kind: DungeonAnchorKind::SafeEntry,
                x_milli_tiles: 3_000,
                y_milli_tiles: 5_500,
                bound_content_id: None,
            }],
            safe_noncombat: true,
        }
        .validated()
        .expect("room")
    }

    #[test]
    fn clockwise_rotation_transforms_dimensions_doors_geometry_and_anchors() {
        let rotated = room().rotated(90).expect("rotation");
        assert_eq!(
            (rotated.width_milli_tiles, rotated.height_milli_tiles),
            (11_000, 13_000)
        );
        assert_eq!(rotated.doors[0].side, DungeonDoorSide::North);
        assert_eq!(
            (
                rotated.doors[0].x_milli_tiles,
                rotated.doors[0].y_milli_tiles
            ),
            (5_500, 0)
        );
        assert_eq!(
            (
                rotated.anchors[0].x_milli_tiles,
                rotated.anchors[0].y_milli_tiles
            ),
            (5_500, 3_000)
        );
        assert_eq!(
            rotated.volumes[0].geometry,
            DungeonRoomVolumeGeometry::Rectangle {
                x: 7_000,
                y: 1_000,
                width: 2_000,
                height: 3_000,
            }
        );
    }

    #[test]
    fn invalid_rotation_and_duplicate_ids_fail_closed() {
        assert_eq!(room().rotated(45), Err(DungeonRoomError::InvalidRotation));
        let mut duplicate = room();
        duplicate.doors.push(duplicate.doors[0].clone());
        assert_eq!(
            duplicate.validated(),
            Err(DungeonRoomError::DuplicateId("door".to_owned()))
        );
    }

    #[test]
    fn navigation_proof_respects_expanded_solids_and_required_anchors() {
        let mut definition = room();
        definition.doors.push(DungeonDoorDefinition {
            id: "east".to_owned(),
            side: DungeonDoorSide::East,
            offset_milli_tiles: 5_500,
            width_milli_tiles: 3_000,
        });
        let evidence = definition
            .prove_navigation(&[DungeonAnchorKind::SafeEntry], 350, 500)
            .expect("connected room");
        assert_eq!(evidence.required_target_count, 3);
        assert!(evidence.visited_grid_points > 100);

        definition.volumes.push(DungeonRoomVolume {
            id: "wall".to_owned(),
            kind: DungeonRoomVolumeKind::Solid,
            geometry: DungeonRoomVolumeGeometry::Rectangle {
                x: 6_000,
                y: 0,
                width: 1_000,
                height: 11_000,
            },
        });
        assert_eq!(
            definition.prove_navigation(&[DungeonAnchorKind::SafeEntry], 350, 500),
            Err(DungeonRoomError::DisconnectedNavigation)
        );
    }
}
