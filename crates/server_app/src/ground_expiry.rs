use persistence::{
    PersistenceError, PostgresPersistence, StoredGroundExpiry, StoredGroundExpiryCandidate,
};
use thiserror::Error;

pub const GROUND_EXPIRY_CONTEXT: &str = "gravebound.ground-expiry.v1";

#[derive(Debug, Clone)]
pub struct PostgresGroundExpiryService {
    persistence: PostgresPersistence,
}

impl PostgresGroundExpiryService {
    pub const fn new(persistence: PostgresPersistence) -> Self {
        Self { persistence }
    }

    pub async fn expire_due(
        &self,
        instance_id: [u8; 16],
        current_tick: u64,
        limit: u16,
    ) -> Result<Vec<StoredGroundExpiry>, GroundExpiryError> {
        let current_tick =
            i64::try_from(current_tick).map_err(|_| GroundExpiryError::InvalidTick)?;
        self.persistence
            .expire_personal_ground(instance_id, current_tick, limit, |candidates| {
                candidates
                    .iter()
                    .map(derive_expiry_event_id)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| PersistenceError::CorruptStoredItems)
            })
            .await
            .map_err(GroundExpiryError::Persistence)
    }
}

#[derive(Debug, Error)]
pub enum GroundExpiryError {
    #[error("ground expiry tick is invalid")]
    InvalidTick,
    #[error("ground expiry persistence failed")]
    Persistence(#[source] PersistenceError),
}

fn derive_expiry_event_id(
    candidate: &StoredGroundExpiryCandidate,
) -> Result<[u8; 16], GroundExpiryError> {
    let expiry_bytes = candidate.expires_at_tick.to_le_bytes();
    let mut material = Vec::new();
    for field in [
        candidate.item_uid.as_slice(),
        candidate.pickup_id.as_slice(),
        expiry_bytes.as_slice(),
    ] {
        let length = u32::try_from(field.len()).map_err(|_| GroundExpiryError::InvalidTick)?;
        material.extend_from_slice(&length.to_le_bytes());
        material.extend_from_slice(field);
    }
    let derived = blake3::derive_key(GROUND_EXPIRY_CONTEXT, &material);
    let mut event_id = [0; 16];
    event_id.copy_from_slice(&derived[..16]);
    if event_id == [0; 16] {
        return Err(GroundExpiryError::InvalidTick);
    }
    Ok(event_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_event_identity_is_stable_and_binds_pickup_and_tick() {
        let mut candidate = StoredGroundExpiryCandidate {
            account_id: [1; 16],
            character_id: [2; 16],
            item_uid: [3; 16],
            pickup_id: [4; 16],
            expires_at_tick: 1_800,
            item_version: 1,
        };
        let event = derive_expiry_event_id(&candidate).unwrap();
        assert_eq!(event, derive_expiry_event_id(&candidate).unwrap());
        candidate.expires_at_tick += 1;
        assert_ne!(event, derive_expiry_event_id(&candidate).unwrap());
        candidate.expires_at_tick -= 1;
        candidate.pickup_id = [5; 16];
        assert_ne!(event, derive_expiry_event_id(&candidate).unwrap());
    }
}
