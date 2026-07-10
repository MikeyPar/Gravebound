use std::collections::BTreeSet;

use thiserror::Error;

/// Fixed-point scale used at the authored-content boundary.
pub const MILLI_TILES_PER_TILE: i32 = 1_000;

/// Exact northwest-origin authored point in milli-tiles.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TilePoint {
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
}

impl TilePoint {
    #[must_use]
    pub const fn new(x_milli_tiles: i32, y_milli_tiles: i32) -> Self {
        Self {
            x_milli_tiles,
            y_milli_tiles,
        }
    }
}

/// Exact northwest-origin authored rectangle in milli-tiles.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileRectangle {
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
    pub width_milli_tiles: i32,
    pub height_milli_tiles: i32,
}

impl TileRectangle {
    #[must_use]
    pub const fn new(
        x_milli_tiles: i32,
        y_milli_tiles: i32,
        width_milli_tiles: i32,
        height_milli_tiles: i32,
    ) -> Self {
        Self {
            x_milli_tiles,
            y_milli_tiles,
            width_milli_tiles,
            height_milli_tiles,
        }
    }

    fn right(self) -> Option<i32> {
        self.x_milli_tiles.checked_add(self.width_milli_tiles)
    }

    fn bottom(self) -> Option<i32> {
        self.y_milli_tiles.checked_add(self.height_milli_tiles)
    }

    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        let (Some(right), Some(bottom), Some(other_right), Some(other_bottom)) =
            (self.right(), self.bottom(), other.right(), other.bottom())
        else {
            return true;
        };
        self.x_milli_tiles < other_right
            && right > other.x_milli_tiles
            && self.y_milli_tiles < other_bottom
            && bottom > other.y_milli_tiles
    }
}

/// Named point consumed by deterministic encounter scheduling and debug presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArenaAnchor {
    pub id: String,
    pub point: TilePoint,
}

/// Simulation-owned immutable arena geometry. Bevy transforms are never stored here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArenaGeometry {
    pub id: String,
    pub width_milli_tiles: i32,
    pub height_milli_tiles: i32,
    pub shell_thickness_milli_tiles: i32,
    pub player_spawn: TilePoint,
    pub boss_spawn: TilePoint,
    pub pillars: Vec<TileRectangle>,
    pub anchors: Vec<ArenaAnchor>,
}

impl ArenaGeometry {
    /// Validates and returns one authoritative geometry object.
    pub fn validated(self) -> Result<Self, ArenaGeometryError> {
        self.validate_invariants()?;
        Ok(self)
    }

    /// Returns the four solid shell rectangles in authored northwest coordinates.
    pub fn shell_rectangles(&self) -> Result<[TileRectangle; 4], ArenaGeometryError> {
        let thickness = self.shell_thickness_milli_tiles;
        let outer_width = self
            .width_milli_tiles
            .checked_add(
                thickness
                    .checked_mul(2)
                    .ok_or(ArenaGeometryError::Overflow)?,
            )
            .ok_or(ArenaGeometryError::Overflow)?;
        Ok([
            TileRectangle::new(-thickness, -thickness, outer_width, thickness),
            TileRectangle::new(-thickness, self.height_milli_tiles, outer_width, thickness),
            TileRectangle::new(-thickness, 0, thickness, self.height_milli_tiles),
            TileRectangle::new(
                self.width_milli_tiles,
                0,
                thickness,
                self.height_milli_tiles,
            ),
        ])
    }

    fn validate_invariants(&self) -> Result<(), ArenaGeometryError> {
        if self.id.trim().is_empty() {
            return Err(ArenaGeometryError::EmptyArenaId);
        }
        if self.width_milli_tiles <= 0 || self.height_milli_tiles <= 0 {
            return Err(ArenaGeometryError::InvalidBounds);
        }
        if self.shell_thickness_milli_tiles <= 0 {
            return Err(ArenaGeometryError::InvalidShellThickness);
        }
        self.validate_point("player_spawn", self.player_spawn)?;
        self.validate_point("boss_spawn", self.boss_spawn)?;

        for (index, pillar) in self.pillars.iter().copied().enumerate() {
            if pillar.width_milli_tiles <= 0 || pillar.height_milli_tiles <= 0 {
                return Err(ArenaGeometryError::InvalidPillar { index });
            }
            let Some(right) = pillar.right() else {
                return Err(ArenaGeometryError::Overflow);
            };
            let Some(bottom) = pillar.bottom() else {
                return Err(ArenaGeometryError::Overflow);
            };
            if pillar.x_milli_tiles < 0
                || pillar.y_milli_tiles < 0
                || right > self.width_milli_tiles
                || bottom > self.height_milli_tiles
            {
                return Err(ArenaGeometryError::PillarOutOfBounds { index });
            }
        }
        for first in 0..self.pillars.len() {
            for second in (first + 1)..self.pillars.len() {
                if self.pillars[first].overlaps(self.pillars[second]) {
                    return Err(ArenaGeometryError::OverlappingPillars { first, second });
                }
            }
        }

        let mut anchor_ids = BTreeSet::new();
        for anchor in &self.anchors {
            if anchor.id.trim().is_empty() {
                return Err(ArenaGeometryError::EmptyAnchorId);
            }
            if !anchor_ids.insert(&anchor.id) {
                return Err(ArenaGeometryError::DuplicateAnchorId(anchor.id.clone()));
            }
            self.validate_point(&anchor.id, anchor.point)?;
        }
        self.shell_rectangles()?;
        Ok(())
    }

    fn validate_point(&self, id: &str, point: TilePoint) -> Result<(), ArenaGeometryError> {
        if point.x_milli_tiles < 0
            || point.y_milli_tiles < 0
            || point.x_milli_tiles > self.width_milli_tiles
            || point.y_milli_tiles > self.height_milli_tiles
        {
            return Err(ArenaGeometryError::PointOutOfBounds(id.to_owned()));
        }
        Ok(())
    }
}

/// Fail-closed geometry compilation error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ArenaGeometryError {
    #[error("arena ID must not be empty")]
    EmptyArenaId,
    #[error("arena width and height must be positive")]
    InvalidBounds,
    #[error("arena shell thickness must be positive")]
    InvalidShellThickness,
    #[error("arena point `{0}` is outside walkable bounds")]
    PointOutOfBounds(String),
    #[error("pillar {index} has nonpositive dimensions")]
    InvalidPillar { index: usize },
    #[error("pillar {index} is outside walkable bounds")]
    PillarOutOfBounds { index: usize },
    #[error("pillars {first} and {second} overlap")]
    OverlappingPillars { first: usize, second: usize },
    #[error("anchor ID must not be empty")]
    EmptyAnchorId,
    #[error("duplicate anchor ID `{0}`")]
    DuplicateAnchorId(String),
    #[error("arena geometry arithmetic overflow")]
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.test".to_owned(),
            width_milli_tiles: 32_000,
            height_milli_tiles: 24_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(4_000, 12_000),
            boss_spawn: TilePoint::new(24_000, 12_000),
            pillars: vec![TileRectangle::new(10_000, 5_000, 2_000, 3_000)],
            anchors: vec![ArenaAnchor {
                id: "N1".to_owned(),
                point: TilePoint::new(8_000, 3_000),
            }],
        }
        .validated()
        .expect("valid sample")
    }

    #[test]
    fn shell_surrounds_walkable_bounds_without_changing_units() {
        assert_eq!(
            sample().shell_rectangles().expect("shell"),
            [
                TileRectangle::new(-1_000, -1_000, 34_000, 1_000),
                TileRectangle::new(-1_000, 24_000, 34_000, 1_000),
                TileRectangle::new(-1_000, 0, 1_000, 24_000),
                TileRectangle::new(32_000, 0, 1_000, 24_000),
            ]
        );
    }

    #[test]
    fn invalid_and_ambiguous_geometry_is_rejected() {
        let mut duplicate = sample();
        duplicate.anchors.push(duplicate.anchors[0].clone());
        assert!(matches!(
            duplicate.validate_invariants(),
            Err(ArenaGeometryError::DuplicateAnchorId(_))
        ));

        let mut overlap = sample();
        overlap
            .pillars
            .push(TileRectangle::new(11_000, 6_000, 2_000, 2_000));
        assert_eq!(
            overlap.validate_invariants(),
            Err(ArenaGeometryError::OverlappingPillars {
                first: 0,
                second: 1
            })
        );
    }
}
