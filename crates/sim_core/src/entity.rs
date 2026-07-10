use std::{fmt, num::NonZeroU64};

use serde::{Deserialize, Serialize};

/// Stable simulation-owned entity identifier, independent of presentation ECS identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(NonZeroU64);

impl EntityId {
    /// Creates an entity ID when the value is nonzero.
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the integer representation used by canonical state encoding.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(formatter)
    }
}

/// Monotonic entity-ID allocator stored as part of simulation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityIdAllocator {
    next: NonZeroU64,
}

impl Default for EntityIdAllocator {
    fn default() -> Self {
        Self {
            next: NonZeroU64::MIN,
        }
    }
}

impl EntityIdAllocator {
    /// Creates an allocator whose next result is the supplied nonzero value.
    #[must_use]
    pub const fn starting_at(next: NonZeroU64) -> Self {
        Self { next }
    }

    /// Allocates the next stable ID, or returns `None` after exhausting `u64`.
    pub fn allocate(&mut self) -> Option<EntityId> {
        let allocated = EntityId(self.next);
        self.next = NonZeroU64::new(self.next.get().checked_add(1)?)?;
        Some(allocated)
    }

    /// Returns the next value without allocating it for canonical hashing.
    #[must_use]
    pub const fn peek(&self) -> EntityId {
        EntityId(self.next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_is_nonzero_and_monotonic() {
        let mut allocator = EntityIdAllocator::default();
        assert_eq!(allocator.allocate().expect("id").get(), 1);
        assert_eq!(allocator.allocate().expect("id").get(), 2);
        assert_eq!(allocator.peek().get(), 3);
    }
}
