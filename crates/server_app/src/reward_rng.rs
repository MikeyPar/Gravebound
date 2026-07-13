use std::fmt;

use rand_chacha::ChaCha8Rng;
use rand_core::{Rng, SeedableRng};
use sim_content::{ProductionRewardDrawSource, ProductionRewardPlanningError};
use thiserror::Error;

pub const REWARD_EPOCH_ID_ENV: &str = "GRAVEBOUND_REWARD_EPOCH_ID";
pub const REWARD_EPOCH_SECRET_ENV: &str = "GRAVEBOUND_REWARD_EPOCH_SECRET_HEX";
pub const REWARD_PLAN_CONTEXT: &str = "gravebound.reward-plan.v1";
pub const REWARD_AUDIT_CONTEXT: &str = "gravebound.reward-audit.v1";
const REWARD_REQUEST_CONTEXT: &str = "gravebound.reward-request.v1";
const EPOCH_SECRET_BYTES: usize = 32;

#[derive(Clone, PartialEq, Eq)]
pub struct SecretRewardEpoch {
    epoch_id: String,
    secret: [u8; EPOCH_SECRET_BYTES],
}

impl SecretRewardEpoch {
    pub fn new(
        epoch_id: impl Into<String>,
        secret: [u8; EPOCH_SECRET_BYTES],
    ) -> Result<Self, RewardRngError> {
        let epoch_id = epoch_id.into();
        if epoch_id.is_empty() || epoch_id.len() > 64 || epoch_id.chars().any(char::is_control) {
            return Err(RewardRngError::InvalidEpochId);
        }
        if secret == [0; EPOCH_SECRET_BYTES] {
            return Err(RewardRngError::ZeroEpochSecret);
        }
        Ok(Self { epoch_id, secret })
    }

    pub fn from_environment() -> Result<Self, RewardRngError> {
        let epoch_id = std::env::var(REWARD_EPOCH_ID_ENV)
            .map_err(|_| RewardRngError::MissingEpochConfiguration)?;
        let encoded = std::env::var(REWARD_EPOCH_SECRET_ENV)
            .map_err(|_| RewardRngError::MissingEpochConfiguration)?;
        Self::new(epoch_id, decode_secret(&encoded)?)
    }

    #[must_use]
    pub fn epoch_id(&self) -> &str {
        &self.epoch_id
    }

    pub fn planner(
        &self,
        material: &RewardSeedMaterial<'_>,
    ) -> Result<ProductionRewardRng, RewardRngError> {
        let fields = material.fields()?;
        let mut seed_fields = Vec::with_capacity(fields.len() + 1);
        seed_fields.push(self.secret.as_slice());
        seed_fields.extend(fields);
        Ok(ProductionRewardRng {
            inner: ChaCha8Rng::from_seed(derive(REWARD_PLAN_CONTEXT, &seed_fields)?),
        })
    }

    pub fn audit_digest(
        &self,
        material: &RewardSeedMaterial<'_>,
        canonical_result: &[u8],
    ) -> Result<[u8; 32], RewardRngError> {
        if canonical_result.is_empty() {
            return Err(RewardRngError::EmptyCanonicalResult);
        }
        let fields = material.fields()?;
        let mut audit_fields = Vec::with_capacity(fields.len() + 2);
        audit_fields.push(self.secret.as_slice());
        audit_fields.extend(fields);
        audit_fields.push(canonical_result);
        derive(REWARD_AUDIT_CONTEXT, &audit_fields)
    }
}

impl fmt::Debug for SecretRewardEpoch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretRewardEpoch")
            .field("epoch_id", &self.epoch_id)
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl fmt::Display for SecretRewardEpoch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "SecretRewardEpoch({}, <redacted>)",
            self.epoch_id
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewardSeedMaterial<'a> {
    pub reward_request_id: [u8; 16],
    pub character_id: [u8; 16],
    pub source_instance_id: [u8; 16],
    pub reward_table_id: &'a str,
    pub content_revision: &'a str,
}

impl RewardSeedMaterial<'_> {
    fn fields(&self) -> Result<Vec<&[u8]>, RewardRngError> {
        if self.reward_request_id == [0; 16]
            || self.character_id == [0; 16]
            || self.source_instance_id == [0; 16]
        {
            return Err(RewardRngError::ZeroIdentity);
        }
        if self.reward_table_id.is_empty()
            || self.reward_table_id.len() > 96
            || self.content_revision.is_empty()
            || self.content_revision.len() > 128
        {
            return Err(RewardRngError::InvalidTextField);
        }
        Ok(vec![
            self.reward_request_id.as_slice(),
            self.character_id.as_slice(),
            self.source_instance_id.as_slice(),
            self.reward_table_id.as_bytes(),
            self.content_revision.as_bytes(),
        ])
    }

    pub fn canonical_request_hash(&self) -> Result<[u8; 32], RewardRngError> {
        derive(REWARD_REQUEST_CONTEXT, &self.fields()?)
    }
}

#[derive(Debug, Clone)]
pub struct ProductionRewardRng {
    inner: ChaCha8Rng,
}

impl ProductionRewardDrawSource for ProductionRewardRng {
    fn draw_below(&mut self, upper_exclusive: u32) -> Result<u32, ProductionRewardPlanningError> {
        if upper_exclusive == 0 {
            return Err(ProductionRewardPlanningError::DrawOutOfRange);
        }
        let rejection_zone = upper_exclusive.wrapping_neg() % upper_exclusive;
        loop {
            let value = self.inner.next_u32();
            if value >= rejection_zone {
                return Ok(value % upper_exclusive);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RewardRngError {
    #[error("active reward epoch configuration is required")]
    MissingEpochConfiguration,
    #[error("reward epoch identifier is invalid")]
    InvalidEpochId,
    #[error("reward epoch secret must be a nonzero 32-byte value")]
    InvalidEpochSecret,
    #[error("reward epoch secret cannot be all zero")]
    ZeroEpochSecret,
    #[error("reward seed contains a zero identity")]
    ZeroIdentity,
    #[error("reward seed text field is invalid")]
    InvalidTextField,
    #[error("reward seed field is too long")]
    FieldTooLong,
    #[error("canonical persisted reward result cannot be empty")]
    EmptyCanonicalResult,
}

fn derive(context: &str, fields: &[&[u8]]) -> Result<[u8; 32], RewardRngError> {
    let mut bytes = Vec::new();
    for field in fields {
        let length = u32::try_from(field.len()).map_err(|_| RewardRngError::FieldTooLong)?;
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(field);
    }
    Ok(blake3::derive_key(context, &bytes))
}

fn decode_secret(encoded: &str) -> Result<[u8; EPOCH_SECRET_BYTES], RewardRngError> {
    if encoded.len() != EPOCH_SECRET_BYTES * 2 || !encoded.is_ascii() {
        return Err(RewardRngError::InvalidEpochSecret);
    }
    let mut secret = [0; EPOCH_SECRET_BYTES];
    for (index, chunk) in encoded.as_bytes().chunks_exact(2).enumerate() {
        secret[index] = (hex_nibble(chunk[0])? << 4) | hex_nibble(chunk[1])?;
    }
    Ok(secret)
}

const fn hex_nibble(value: u8) -> Result<u8, RewardRngError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(RewardRngError::InvalidEpochSecret),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material<'a>() -> RewardSeedMaterial<'a> {
        RewardSeedMaterial {
            reward_request_id: [1; 16],
            character_id: [2; 16],
            source_instance_id: [3; 16],
            reward_table_id: "reward.normal_outer",
            content_revision: CORE_REVISION,
        }
    }

    const CORE_REVISION: &str =
        "core-dev.blake3.27818db710b7553520a162f6f8337dcd0419c459d20c6513a7e12c78fed24ebb";

    #[test]
    fn reward_stream_and_audit_are_stable_and_domain_separated() {
        let epoch = SecretRewardEpoch::new("test-epoch-1", [0x5a; 32]).unwrap();
        let mut first = epoch.planner(&material()).unwrap();
        let mut second = epoch.planner(&material()).unwrap();
        let draws = [
            first.draw_below(10_000).unwrap(),
            first.draw_below(17).unwrap(),
            first.draw_below(u32::MAX).unwrap(),
        ];
        assert_eq!(
            draws,
            [
                second.draw_below(10_000).unwrap(),
                second.draw_below(17).unwrap(),
                second.draw_below(u32::MAX).unwrap(),
            ]
        );
        assert_eq!(draws, [5_245, 15, 254_068_401]);
        assert_ne!(
            epoch
                .audit_digest(&material(), b"persisted-result")
                .unwrap(),
            material().canonical_request_hash().unwrap()
        );
    }

    #[test]
    fn secret_is_redacted_and_configuration_fails_closed() {
        let epoch = SecretRewardEpoch::new("test-epoch-1", [0xa5; 32]).unwrap();
        assert!(!format!("{epoch:?}{epoch}").contains("a5a5"));
        assert_eq!(
            SecretRewardEpoch::new("test", [0; 32]),
            Err(RewardRngError::ZeroEpochSecret)
        );
        assert_eq!(
            decode_secret("not-a-secret"),
            Err(RewardRngError::InvalidEpochSecret)
        );
    }
}
