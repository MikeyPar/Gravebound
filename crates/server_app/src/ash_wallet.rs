//! Authoritative minimal Ash Shard wallet for `GB-M03-12`.

use persistence::{
    ASH_WALLET_CAP, AshMutationCode as StoredAshCode, AshMutationKind as StoredAshKind,
    AshMutationRequest, AshWalletTransaction, PersistenceError, PostgresPersistence,
};
use serde::Serialize;
use thiserror::Error;

use crate::{AuthenticatedAccount, AuthenticatedNamespace, IdentityClock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AshWalletMutationKind {
    Earn,
    Spend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AshReasonCode {
    SalvageTier1,
    SalvageTier2,
    SalvageTier3,
    SalvageTier4,
    MinorRealmEvent,
    MajorRealmEvent,
    BellSepulcherBoss,
    RootChapelBoss,
    DrownedReliquaryBoss,
    BellWarden,
    BargainReplacement,
    AllBossUniquesOwned,
    Achievement,
    Tier2HallContract,
    OathChange,
    BargainPurge,
    ForgeTier1,
    ForgeTier2,
    ForgeTier3,
    TemperTier1,
    TemperTier2,
    TemperTier3,
    TemperTier4,
    ReforgeTier1,
    ReforgeTier2,
    ReforgeTier3,
    ReforgeTier4,
}

impl AshReasonCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SalvageTier1 => "salvage_tier_1",
            Self::SalvageTier2 => "salvage_tier_2",
            Self::SalvageTier3 => "salvage_tier_3",
            Self::SalvageTier4 => "salvage_tier_4",
            Self::MinorRealmEvent => "minor_realm_event",
            Self::MajorRealmEvent => "major_realm_event",
            Self::BellSepulcherBoss => "bell_sepulcher_boss",
            Self::RootChapelBoss => "root_chapel_boss",
            Self::DrownedReliquaryBoss => "drowned_reliquary_boss",
            Self::BellWarden => "bell_warden",
            Self::BargainReplacement => "bargain_replacement",
            Self::AllBossUniquesOwned => "all_boss_uniques_owned",
            Self::Achievement => "achievement",
            Self::Tier2HallContract => "tier_2_hall_contract",
            Self::OathChange => "oath_change",
            Self::BargainPurge => "bargain_purge",
            Self::ForgeTier1 => "forge_tier_1",
            Self::ForgeTier2 => "forge_tier_2",
            Self::ForgeTier3 => "forge_tier_3",
            Self::TemperTier1 => "temper_tier_1",
            Self::TemperTier2 => "temper_tier_2",
            Self::TemperTier3 => "temper_tier_3",
            Self::TemperTier4 => "temper_tier_4",
            Self::ReforgeTier1 => "reforge_tier_1",
            Self::ReforgeTier2 => "reforge_tier_2",
            Self::ReforgeTier3 => "reforge_tier_3",
            Self::ReforgeTier4 => "reforge_tier_4",
        }
    }

    const fn contract(self) -> (AshWalletMutationKind, Option<u32>) {
        match self {
            Self::SalvageTier1 => (AshWalletMutationKind::Earn, Some(4)),
            Self::SalvageTier2 | Self::BellSepulcherBoss => (AshWalletMutationKind::Earn, Some(12)),
            Self::SalvageTier3 => (AshWalletMutationKind::Earn, Some(36)),
            Self::SalvageTier4 | Self::AllBossUniquesOwned => {
                (AshWalletMutationKind::Earn, Some(80))
            }
            Self::MinorRealmEvent | Self::BargainReplacement => {
                (AshWalletMutationKind::Earn, Some(10))
            }
            Self::MajorRealmEvent => (AshWalletMutationKind::Earn, Some(25)),
            Self::RootChapelBoss => (AshWalletMutationKind::Earn, Some(24)),
            Self::DrownedReliquaryBoss => (AshWalletMutationKind::Earn, Some(40)),
            Self::BellWarden => (AshWalletMutationKind::Earn, Some(50)),
            Self::Achievement => (AshWalletMutationKind::Earn, None),
            Self::Tier2HallContract | Self::ForgeTier1 => (AshWalletMutationKind::Spend, Some(20)),
            Self::OathChange => (AshWalletMutationKind::Spend, Some(40)),
            Self::BargainPurge => (AshWalletMutationKind::Spend, Some(50)),
            Self::ForgeTier2 => (AshWalletMutationKind::Spend, Some(60)),
            Self::ForgeTier3 => (AshWalletMutationKind::Spend, Some(120)),
            Self::TemperTier1 => (AshWalletMutationKind::Spend, Some(8)),
            Self::TemperTier2 => (AshWalletMutationKind::Spend, Some(24)),
            Self::TemperTier3 => (AshWalletMutationKind::Spend, Some(72)),
            Self::TemperTier4 => (AshWalletMutationKind::Spend, Some(160)),
            Self::ReforgeTier1 => (AshWalletMutationKind::Spend, Some(16)),
            Self::ReforgeTier2 => (AshWalletMutationKind::Spend, Some(48)),
            Self::ReforgeTier3 => (AshWalletMutationKind::Spend, Some(144)),
            Self::ReforgeTier4 => (AshWalletMutationKind::Spend, Some(320)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AshWalletMutationPayload {
    pub kind: AshWalletMutationKind,
    pub reason: AshReasonCode,
    pub amount: u32,
    pub source_id: String,
    pub content_version: String,
}

impl AshWalletMutationPayload {
    pub fn canonical_hash(&self) -> [u8; 32] {
        let bytes = postcard::to_stdvec(self).expect("bounded Ash payload serializes");
        *blake3::hash(&bytes).as_bytes()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AshWalletMutationFrame {
    pub mutation_id: [u8; 16],
    pub expected_wallet_version: u64,
    pub payload_hash: [u8; 32],
    pub issued_at_unix_millis: u64,
    pub payload: AshWalletMutationPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AshWalletProjection {
    pub balance: u32,
    pub wallet_version: u64,
    pub cap: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AshWalletResultCode {
    Accepted,
    InsufficientBalance,
    CapExceeded,
    StateVersionMismatch,
    IdempotencyConflict,
    InvalidRequest,
    ServiceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AshWalletMutationResult {
    pub mutation_id: [u8; 16],
    pub code: AshWalletResultCode,
    pub projection: Option<AshWalletProjection>,
}

#[derive(Debug, Clone)]
pub struct PostgresAshWalletService<Clock> {
    persistence: PostgresPersistence,
    clock: Clock,
}

impl<Clock> PostgresAshWalletService<Clock>
where
    Clock: IdentityClock,
{
    pub const fn new(persistence: PostgresPersistence, clock: Clock) -> Self {
        Self { persistence, clock }
    }

    pub async fn view(
        &self,
        authenticated: AuthenticatedAccount,
    ) -> Result<AshWalletProjection, AshWalletServiceError> {
        require_test_namespace(authenticated)?;
        let stored = self
            .persistence
            .ash_wallet_snapshot(authenticated.account_id.as_bytes())
            .await
            .map_err(|_| AshWalletServiceError::Unavailable)?;
        stored.map_or_else(
            || projection(0, 1),
            |wallet| projection(wallet.balance, wallet.wallet_version),
        )
    }

    pub async fn mutate(
        &self,
        authenticated: AuthenticatedAccount,
        frame: &AshWalletMutationFrame,
    ) -> AshWalletMutationResult {
        if require_test_namespace(authenticated).is_err()
            || validate_frame(frame).is_err()
            || frame.issued_at_unix_millis > self.clock.unix_millis()
        {
            return result(frame.mutation_id, AshWalletResultCode::InvalidRequest, None);
        }
        let Ok(expected_wallet_version) = i64::try_from(frame.expected_wallet_version) else {
            return result(frame.mutation_id, AshWalletResultCode::InvalidRequest, None);
        };
        let Ok(amount) = i32::try_from(frame.payload.amount) else {
            return result(frame.mutation_id, AshWalletResultCode::InvalidRequest, None);
        };
        let request = AshMutationRequest {
            account_id: authenticated.account_id.as_bytes(),
            mutation_id: frame.mutation_id,
            payload_hash: frame.payload_hash,
            expected_wallet_version,
            kind: match frame.payload.kind {
                AshWalletMutationKind::Earn => StoredAshKind::Earn,
                AshWalletMutationKind::Spend => StoredAshKind::Spend,
            },
            amount,
            reason_code: frame.payload.reason.as_str().into(),
            source_id: frame.payload.source_id.clone(),
            content_version: frame.payload.content_version.clone(),
        };
        match self.persistence.transact_ash_mutation(&request).await {
            Ok(
                AshWalletTransaction::Committed(stored) | AshWalletTransaction::Replayed(stored),
            ) => from_stored(frame.mutation_id, &stored),
            Err(PersistenceError::AshIdempotencyConflict) => result(
                frame.mutation_id,
                AshWalletResultCode::IdempotencyConflict,
                None,
            ),
            Err(_) => result(
                frame.mutation_id,
                AshWalletResultCode::ServiceUnavailable,
                None,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AshWalletServiceError {
    #[error("Ash wallet is unavailable outside the wipeable Core namespace")]
    Namespace,
    #[error("Ash wallet persistence is unavailable")]
    Unavailable,
    #[error("stored Ash wallet projection is invalid")]
    InvalidProjection,
}

fn require_test_namespace(
    authenticated: AuthenticatedAccount,
) -> Result<(), AshWalletServiceError> {
    if authenticated.namespace == AuthenticatedNamespace::WipeableTest {
        Ok(())
    } else {
        Err(AshWalletServiceError::Namespace)
    }
}

fn validate_frame(frame: &AshWalletMutationFrame) -> Result<(), ()> {
    let (required_kind, required_amount) = frame.payload.reason.contract();
    let valid_amount = required_amount.map_or_else(
        || {
            frame.payload.reason == AshReasonCode::Achievement
                && (1..=100).contains(&frame.payload.amount)
        },
        |amount| amount == frame.payload.amount,
    );
    if frame.mutation_id == [0; 16]
        || frame.expected_wallet_version == 0
        || frame.payload_hash == [0; 32]
        || frame.issued_at_unix_millis == 0
        || frame.payload_hash != frame.payload.canonical_hash()
        || frame.payload.kind != required_kind
        || !valid_amount
        || !bounded(&frame.payload.source_id, 128)
        || !bounded(&frame.payload.content_version, 128)
        || !frame
            .payload
            .content_version
            .starts_with("core-dev.blake3.")
    {
        return Err(());
    }
    Ok(())
}

fn bounded(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.chars().count() <= maximum
}

fn from_stored(
    mutation_id: [u8; 16],
    stored: &persistence::StoredAshMutationResult,
) -> AshWalletMutationResult {
    let code = match stored.code {
        StoredAshCode::Accepted => AshWalletResultCode::Accepted,
        StoredAshCode::InsufficientBalance => AshWalletResultCode::InsufficientBalance,
        StoredAshCode::CapExceeded => AshWalletResultCode::CapExceeded,
        StoredAshCode::StateVersionMismatch => AshWalletResultCode::StateVersionMismatch,
    };
    let projection = u32::try_from(stored.after_balance)
        .ok()
        .zip(u64::try_from(stored.post_wallet_version).ok())
        .and_then(|(balance, version)| projection(balance, version).ok());
    result(mutation_id, code, projection)
}

fn projection(
    balance: impl TryInto<u32>,
    version: impl TryInto<u64>,
) -> Result<AshWalletProjection, AshWalletServiceError> {
    let balance = balance
        .try_into()
        .map_err(|_| AshWalletServiceError::InvalidProjection)?;
    let wallet_version = version
        .try_into()
        .map_err(|_| AshWalletServiceError::InvalidProjection)?;
    let cap = u32::try_from(ASH_WALLET_CAP).expect("Ash cap fits u32");
    if balance > cap || wallet_version == 0 {
        return Err(AshWalletServiceError::InvalidProjection);
    }
    Ok(AshWalletProjection {
        balance,
        wallet_version,
        cap,
    })
}

const fn result(
    mutation_id: [u8; 16],
    code: AshWalletResultCode,
    projection: Option<AshWalletProjection>,
) -> AshWalletMutationResult {
    AshWalletMutationResult {
        mutation_id,
        code,
        projection,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        reason: AshReasonCode,
        kind: AshWalletMutationKind,
        amount: u32,
    ) -> AshWalletMutationFrame {
        let payload = AshWalletMutationPayload {
            kind,
            reason,
            amount,
            source_id: "source.test".into(),
            content_version: format!("core-dev.blake3.{}", "a".repeat(64)),
        };
        AshWalletMutationFrame {
            mutation_id: [1; 16],
            expected_wallet_version: 1,
            payload_hash: payload.canonical_hash(),
            issued_at_unix_millis: 1,
            payload,
        }
    }

    #[test]
    fn every_exact_source_and_sink_contract_is_pinned() {
        for (reason, kind, amount) in [
            (AshReasonCode::SalvageTier1, AshWalletMutationKind::Earn, 4),
            (AshReasonCode::SalvageTier2, AshWalletMutationKind::Earn, 12),
            (AshReasonCode::SalvageTier3, AshWalletMutationKind::Earn, 36),
            (AshReasonCode::SalvageTier4, AshWalletMutationKind::Earn, 80),
            (
                AshReasonCode::MinorRealmEvent,
                AshWalletMutationKind::Earn,
                10,
            ),
            (
                AshReasonCode::MajorRealmEvent,
                AshWalletMutationKind::Earn,
                25,
            ),
            (
                AshReasonCode::BellSepulcherBoss,
                AshWalletMutationKind::Earn,
                12,
            ),
            (
                AshReasonCode::RootChapelBoss,
                AshWalletMutationKind::Earn,
                24,
            ),
            (
                AshReasonCode::DrownedReliquaryBoss,
                AshWalletMutationKind::Earn,
                40,
            ),
            (AshReasonCode::BellWarden, AshWalletMutationKind::Earn, 50),
            (
                AshReasonCode::BargainReplacement,
                AshWalletMutationKind::Earn,
                10,
            ),
            (
                AshReasonCode::AllBossUniquesOwned,
                AshWalletMutationKind::Earn,
                80,
            ),
            (
                AshReasonCode::Tier2HallContract,
                AshWalletMutationKind::Spend,
                20,
            ),
            (AshReasonCode::OathChange, AshWalletMutationKind::Spend, 40),
            (
                AshReasonCode::BargainPurge,
                AshWalletMutationKind::Spend,
                50,
            ),
            (AshReasonCode::ForgeTier1, AshWalletMutationKind::Spend, 20),
            (AshReasonCode::ForgeTier2, AshWalletMutationKind::Spend, 60),
            (AshReasonCode::ForgeTier3, AshWalletMutationKind::Spend, 120),
            (AshReasonCode::TemperTier1, AshWalletMutationKind::Spend, 8),
            (AshReasonCode::TemperTier2, AshWalletMutationKind::Spend, 24),
            (AshReasonCode::TemperTier3, AshWalletMutationKind::Spend, 72),
            (
                AshReasonCode::TemperTier4,
                AshWalletMutationKind::Spend,
                160,
            ),
            (
                AshReasonCode::ReforgeTier1,
                AshWalletMutationKind::Spend,
                16,
            ),
            (
                AshReasonCode::ReforgeTier2,
                AshWalletMutationKind::Spend,
                48,
            ),
            (
                AshReasonCode::ReforgeTier3,
                AshWalletMutationKind::Spend,
                144,
            ),
            (
                AshReasonCode::ReforgeTier4,
                AshWalletMutationKind::Spend,
                320,
            ),
        ] {
            assert_eq!(validate_frame(&frame(reason, kind, amount)), Ok(()));
        }
        for amount in [1, 50, 100] {
            assert_eq!(
                validate_frame(&frame(
                    AshReasonCode::Achievement,
                    AshWalletMutationKind::Earn,
                    amount,
                )),
                Ok(())
            );
        }
    }

    #[test]
    fn wrong_amount_kind_hash_version_or_content_fails_closed() {
        for amount in [0, 101] {
            assert_eq!(
                validate_frame(&frame(
                    AshReasonCode::Achievement,
                    AshWalletMutationKind::Earn,
                    amount,
                )),
                Err(())
            );
        }
        assert_eq!(
            validate_frame(&frame(
                AshReasonCode::OathChange,
                AshWalletMutationKind::Spend,
                39
            )),
            Err(())
        );
        assert_eq!(
            validate_frame(&frame(
                AshReasonCode::OathChange,
                AshWalletMutationKind::Earn,
                40
            )),
            Err(())
        );
        let mut invalid = frame(
            AshReasonCode::MinorRealmEvent,
            AshWalletMutationKind::Earn,
            10,
        );
        invalid.payload_hash = [2; 32];
        assert_eq!(validate_frame(&invalid), Err(()));
        invalid = frame(
            AshReasonCode::MinorRealmEvent,
            AshWalletMutationKind::Earn,
            10,
        );
        invalid.expected_wallet_version = 0;
        assert_eq!(validate_frame(&invalid), Err(()));
        invalid = frame(
            AshReasonCode::MinorRealmEvent,
            AshWalletMutationKind::Earn,
            10,
        );
        invalid.payload.content_version = "core.1.0.0".into();
        invalid.payload_hash = invalid.payload.canonical_hash();
        assert_eq!(validate_frame(&invalid), Err(()));
    }
}
