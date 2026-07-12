//! Ephemeral Core account/character authority for `GB-M03-01B`.
//!
//! The adapter is intentionally process-local and wipeable. Domain validation stays in
//! [`IdentityService`], while [`AccountRepository`] exposes the single-writer transaction seam
//! that a later durable adapter can implement without moving rules into SQL.

use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    future::Future,
    sync::{Arc, Mutex},
};

use persistence::{
    PersistenceError, PostgresPersistence, StoredCharacter, StoredIdentityAggregate, StoredMutation,
};
use protocol::{
    AccountBootstrapFrame, AccountBootstrapResult, AccountErrorCode, AccountNamespace,
    AccountSnapshot, CHARACTER_ID_BYTES, CORE_CHARACTER_SLOT_CAPACITY, CharacterLifeState,
    CharacterLocation, CharacterLocationSnapshot, CharacterMutationFrame, CharacterMutationPayload,
    CharacterMutationResult, CharacterSecurityState, CharacterSnapshot, GRAVE_ARBALIST_CLASS_ID,
    MUTATION_ID_BYTES, ManifestHash, WireText,
};
use thiserror::Error;

pub const MAX_ACCOUNT_MUTATION_RESULTS: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId([u8; 16]);

impl AccountId {
    pub const fn new(bytes: [u8; 16]) -> Option<Self> {
        if all_zero(&bytes) {
            None
        } else {
            Some(Self(bytes))
        }
    }

    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Debug for AccountId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountId(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticatedNamespace {
    WipeableTest,
    Production,
}

/// Identity resolved by the authentication boundary. Raw credentials and platform identifiers
/// never enter this domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthenticatedAccount {
    pub account_id: AccountId,
    pub namespace: AuthenticatedNamespace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CharacterRecord {
    id: [u8; CHARACTER_ID_BYTES],
    roster_ordinal: u8,
    class_id: WireText<96>,
    level: u16,
    oath_id: Option<WireText<96>>,
    life_state: CharacterLifeState,
    security_state: CharacterSecurityState,
    state_version: u64,
}

impl CharacterRecord {
    fn snapshot(&self) -> CharacterSnapshot {
        CharacterSnapshot {
            character_id: self.id,
            roster_ordinal: self.roster_ordinal,
            class_id: self.class_id.clone(),
            level: self.level,
            oath_id: self.oath_id.clone(),
            life_state: self.life_state,
            security_state: self.security_state,
        }
    }
}

#[derive(Debug, Clone)]
struct CachedMutation {
    mutation_id: [u8; MUTATION_ID_BYTES],
    payload_hash: [u8; 32],
    result: CharacterMutationResult,
}

/// Aggregate persisted by an account repository. Fields remain private so adapters cannot bypass
/// the domain service's invariants.
#[derive(Debug, Clone)]
pub struct AccountAggregate {
    version: u64,
    characters: Vec<CharacterRecord>,
    selected_character_id: Option<[u8; CHARACTER_ID_BYTES]>,
    mutations: VecDeque<CachedMutation>,
}

impl AccountAggregate {
    fn new() -> Self {
        Self {
            version: 1,
            characters: Vec::with_capacity(usize::from(CORE_CHARACTER_SLOT_CAPACITY)),
            selected_character_id: None,
            mutations: VecDeque::with_capacity(MAX_ACCOUNT_MUTATION_RESULTS),
        }
    }

    fn snapshot(&self) -> AccountSnapshot {
        AccountSnapshot {
            namespace: AccountNamespace::WipeableTest,
            account_version: self.version,
            slot_capacity: CORE_CHARACTER_SLOT_CAPACITY,
            characters: self
                .characters
                .iter()
                .map(CharacterRecord::snapshot)
                .collect(),
            selected_character_id: self.selected_character_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AccountRepositoryError {
    #[error("account repository is unavailable")]
    Unavailable,
}

/// Single-writer aggregate interface. The operation executes under one account writer lock.
pub trait AccountRepository: Send + Sync {
    fn transact<T, F>(
        &self,
        account_id: AccountId,
        operation: F,
    ) -> impl Future<Output = Result<T, AccountRepositoryError>> + Send
    where
        T: Send,
        F: FnOnce(&mut AccountAggregate) -> T + Send;

    fn character_owner(
        &self,
        character_id: [u8; CHARACTER_ID_BYTES],
    ) -> impl Future<Output = Result<Option<AccountId>, AccountRepositoryError>> + Send;
}

/// Wipeable adapter. Dropping this value (or restarting the process) destroys every aggregate.
#[derive(Debug, Clone, Default)]
pub struct InMemoryAccountRepository {
    accounts: Arc<Mutex<BTreeMap<AccountId, AccountAggregate>>>,
}

impl AccountRepository for InMemoryAccountRepository {
    async fn transact<T, F>(
        &self,
        account_id: AccountId,
        operation: F,
    ) -> Result<T, AccountRepositoryError>
    where
        T: Send,
        F: FnOnce(&mut AccountAggregate) -> T + Send,
    {
        let mut accounts = self
            .accounts
            .lock()
            .map_err(|_| AccountRepositoryError::Unavailable)?;
        let aggregate = accounts
            .entry(account_id)
            .or_insert_with(AccountAggregate::new);
        Ok(operation(aggregate))
    }

    async fn character_owner(
        &self,
        character_id: [u8; CHARACTER_ID_BYTES],
    ) -> Result<Option<AccountId>, AccountRepositoryError> {
        let accounts = self
            .accounts
            .lock()
            .map_err(|_| AccountRepositoryError::Unavailable)?;
        Ok(accounts.iter().find_map(|(account_id, account)| {
            account
                .characters
                .iter()
                .any(|character| character.id == character_id)
                .then_some(*account_id)
        }))
    }
}

impl crate::WorldFlowLocationRepository for InMemoryAccountRepository {
    async fn selected_character(
        &self,
        account_id: AccountId,
    ) -> Result<Option<[u8; CHARACTER_ID_BYTES]>, crate::WorldFlowRepositoryError> {
        let accounts = self
            .accounts
            .lock()
            .map_err(|_| crate::WorldFlowRepositoryError::Unavailable)?;
        Ok(accounts
            .get(&account_id)
            .and_then(|account| account.selected_character_id))
    }

    async fn character_owner(
        &self,
        character_id: [u8; CHARACTER_ID_BYTES],
    ) -> Result<Option<AccountId>, crate::WorldFlowRepositoryError> {
        AccountRepository::character_owner(self, character_id)
            .await
            .map_err(|_| crate::WorldFlowRepositoryError::Unavailable)
    }

    async fn location(
        &self,
        account_id: AccountId,
        character_id: [u8; CHARACTER_ID_BYTES],
    ) -> Result<Option<CharacterLocationSnapshot>, crate::WorldFlowRepositoryError> {
        let accounts = self
            .accounts
            .lock()
            .map_err(|_| crate::WorldFlowRepositoryError::Unavailable)?;
        Ok(accounts.get(&account_id).and_then(|account| {
            account.characters.iter().find_map(|character| {
                (character.id == character_id).then_some(CharacterLocationSnapshot {
                    character_id,
                    character_version: character.state_version,
                    location: CharacterLocation::CharacterSelect {
                        next_hall_arrival: protocol::SafeArrival::HallDefault,
                    },
                })
            })
        }))
    }
}

/// Durable adapter backed by the `persistence` crate's serializable identity transaction.
#[derive(Debug, Clone)]
pub struct PostgresAccountRepository {
    persistence: PostgresPersistence,
}

impl PostgresAccountRepository {
    pub const fn new(persistence: PostgresPersistence) -> Self {
        Self { persistence }
    }

    pub(crate) fn persistence(&self) -> PostgresPersistence {
        self.persistence.clone()
    }
}

impl AccountRepository for PostgresAccountRepository {
    async fn transact<T, F>(
        &self,
        account_id: AccountId,
        operation: F,
    ) -> Result<T, AccountRepositoryError>
    where
        T: Send,
        F: FnOnce(&mut AccountAggregate) -> T + Send,
    {
        self.persistence
            .transact_identity(
                account_id.as_bytes(),
                1,
                i16::from(CORE_CHARACTER_SLOT_CAPACITY),
                |stored| {
                    let mut aggregate = AccountAggregate::try_from_stored(stored)?;
                    let result = operation(&mut aggregate);
                    *stored = aggregate.into_stored()?;
                    Ok(result)
                },
            )
            .await
            .map_err(|_| AccountRepositoryError::Unavailable)
    }

    async fn character_owner(
        &self,
        character_id: [u8; CHARACTER_ID_BYTES],
    ) -> Result<Option<AccountId>, AccountRepositoryError> {
        self.persistence
            .identity_character_owner(character_id)
            .await
            .map(|owner| owner.and_then(AccountId::new))
            .map_err(|_| AccountRepositoryError::Unavailable)
    }
}

impl AccountAggregate {
    fn try_from_stored(stored: &StoredIdentityAggregate) -> Result<Self, PersistenceError> {
        let version = u64::try_from(stored.state_version)
            .map_err(|_| PersistenceError::CorruptStoredIdentity)?;
        if stored.slot_capacity != i16::from(CORE_CHARACTER_SLOT_CAPACITY) {
            return Err(PersistenceError::CorruptStoredIdentity);
        }
        let characters = stored
            .characters
            .iter()
            .map(CharacterRecord::try_from_stored)
            .collect::<Result<Vec<_>, _>>()?;
        let mutations = stored
            .mutations
            .iter()
            .map(|stored| {
                let result: CharacterMutationResult = postcard::from_bytes(&stored.result_payload)
                    .map_err(|_| PersistenceError::CorruptStoredIdentity)?;
                if result.mutation_id != stored.mutation_id || result.validate().is_err() {
                    return Err(PersistenceError::CorruptStoredIdentity);
                }
                Ok(CachedMutation {
                    mutation_id: stored.mutation_id,
                    payload_hash: stored.payload_hash,
                    result,
                })
            })
            .collect::<Result<VecDeque<_>, _>>()?;
        let aggregate = Self {
            version,
            characters,
            selected_character_id: stored.selected_character_id,
            mutations,
        };
        aggregate
            .snapshot()
            .validate()
            .map_err(|_| PersistenceError::CorruptStoredIdentity)?;
        Ok(aggregate)
    }

    fn into_stored(self) -> Result<StoredIdentityAggregate, PersistenceError> {
        Ok(StoredIdentityAggregate {
            state_version: i64::try_from(self.version)
                .map_err(|_| PersistenceError::CorruptStoredIdentity)?,
            slot_capacity: i16::from(CORE_CHARACTER_SLOT_CAPACITY),
            selected_character_id: self.selected_character_id,
            characters: self
                .characters
                .into_iter()
                .map(CharacterRecord::into_stored)
                .collect::<Result<Vec<_>, PersistenceError>>()?,
            mutations: self
                .mutations
                .into_iter()
                .map(|cached| {
                    let result_payload = postcard::to_stdvec(&cached.result)
                        .map_err(|_| PersistenceError::CorruptStoredIdentity)?;
                    Ok(StoredMutation {
                        mutation_id: cached.mutation_id,
                        payload_hash: cached.payload_hash,
                        result_payload,
                    })
                })
                .collect::<Result<Vec<_>, PersistenceError>>()?,
        })
    }
}

impl CharacterRecord {
    fn try_from_stored(stored: &StoredCharacter) -> Result<Self, PersistenceError> {
        if stored.life_state != 0 || stored.security_state != 0 {
            return Err(PersistenceError::CorruptStoredIdentity);
        }
        let state_version = u64::try_from(stored.character_state_version)
            .map_err(|_| PersistenceError::CorruptStoredIdentity)?;
        if state_version == 0 {
            return Err(PersistenceError::CorruptStoredIdentity);
        }
        Ok(Self {
            id: stored.character_id,
            roster_ordinal: u8::try_from(stored.roster_ordinal)
                .map_err(|_| PersistenceError::CorruptStoredIdentity)?,
            class_id: WireText::new(&stored.class_id)
                .map_err(|_| PersistenceError::CorruptStoredIdentity)?,
            level: u16::try_from(stored.level)
                .map_err(|_| PersistenceError::CorruptStoredIdentity)?,
            oath_id: stored
                .oath_id
                .as_deref()
                .map(WireText::new)
                .transpose()
                .map_err(|_| PersistenceError::CorruptStoredIdentity)?,
            life_state: CharacterLifeState::Living,
            security_state: CharacterSecurityState::SafeCharacterSelect,
            state_version,
        })
    }

    fn into_stored(self) -> Result<StoredCharacter, PersistenceError> {
        Ok(StoredCharacter {
            character_id: self.id,
            roster_ordinal: i16::from(self.roster_ordinal),
            class_id: self.class_id.as_str().to_owned(),
            level: i32::from(self.level),
            oath_id: self.oath_id.map(|oath| oath.as_str().to_owned()),
            life_state: 0,
            security_state: 0,
            character_state_version: i64::try_from(self.state_version)
                .map_err(|_| PersistenceError::CorruptStoredIdentity)?,
        })
    }
}

pub trait IdentityClock: Send + Sync {
    fn unix_millis(&self) -> u64;
}

pub trait CharacterIdGenerator: Send + Sync {
    fn next_id(&self) -> [u8; CHARACTER_ID_BYTES];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityEvent {
    AccountBootstrapped,
    CharacterCreated { roster_ordinal: u8 },
    CharacterSelected { roster_ordinal: u8 },
    MutationRejected { code: AccountErrorCode },
}

pub trait IdentityEventSink: Send + Sync {
    fn record(&self, event: IdentityEvent);
}

#[derive(Debug, Default)]
pub struct NoopIdentityEventSink;

impl IdentityEventSink for NoopIdentityEventSink {
    fn record(&self, _event: IdentityEvent) {}
}

pub struct IdentityService<R, C, G, E> {
    repository: R,
    clock: C,
    id_generator: G,
    events: E,
    required_manifest_hash: ManifestHash,
}

impl<R, C, G, E> IdentityService<R, C, G, E>
where
    R: AccountRepository,
    C: IdentityClock,
    G: CharacterIdGenerator,
    E: IdentityEventSink,
{
    pub const fn new(
        repository: R,
        clock: C,
        id_generator: G,
        events: E,
        required_manifest_hash: ManifestHash,
    ) -> Self {
        Self {
            repository,
            clock,
            id_generator,
            events,
            required_manifest_hash,
        }
    }

    pub async fn bootstrap(
        &self,
        authenticated: Option<AuthenticatedAccount>,
        frame: &AccountBootstrapFrame,
    ) -> AccountBootstrapResult {
        if frame.validate().is_err() {
            return AccountBootstrapResult::Error(AccountErrorCode::ServiceUnavailable);
        }
        let account = match Self::authorize(authenticated) {
            Ok(account) => account,
            Err(code) => return AccountBootstrapResult::Error(code),
        };
        if frame.content_manifest_hash != self.required_manifest_hash {
            return AccountBootstrapResult::Error(AccountErrorCode::ContentMismatch);
        }
        match self
            .repository
            .transact(account.account_id, |aggregate| aggregate.snapshot())
            .await
        {
            Ok(snapshot) => {
                self.events.record(IdentityEvent::AccountBootstrapped);
                AccountBootstrapResult::Snapshot(snapshot)
            }
            Err(_) => AccountBootstrapResult::Error(AccountErrorCode::ServiceUnavailable),
        }
    }

    pub async fn mutate(
        &self,
        authenticated: Option<AuthenticatedAccount>,
        frame: &CharacterMutationFrame,
    ) -> CharacterMutationResult {
        let account = match Self::authorize(authenticated) {
            Ok(account) => account,
            Err(code) => return result_without_snapshot(frame.mutation_id, code),
        };
        if frame.validate().is_err() {
            return result_without_snapshot(
                frame.mutation_id,
                AccountErrorCode::ServiceUnavailable,
            );
        }
        if frame.payload_hash != frame.payload.canonical_hash() {
            return result_without_snapshot(
                frame.mutation_id,
                AccountErrorCode::PayloadHashMismatch,
            );
        }
        if frame.issued_at_unix_millis > self.clock.unix_millis() {
            return result_without_snapshot(frame.mutation_id, AccountErrorCode::IssuedAtInvalid);
        }

        let foreign_owner = match frame.payload {
            CharacterMutationPayload::Select { character_id } => {
                match self.repository.character_owner(character_id).await {
                    Ok(owner) => owner.filter(|owner| *owner != account.account_id),
                    Err(_) => {
                        return result_without_snapshot(
                            frame.mutation_id,
                            AccountErrorCode::ServiceUnavailable,
                        );
                    }
                }
            }
            CharacterMutationPayload::Create { .. } => None,
        };

        let result = self
            .repository
            .transact(account.account_id, |aggregate| {
                self.apply_mutation(aggregate, frame, foreign_owner.is_some())
            })
            .await;
        result.unwrap_or_else(|_| {
            result_without_snapshot(frame.mutation_id, AccountErrorCode::ServiceUnavailable)
        })
    }

    fn authorize(
        authenticated: Option<AuthenticatedAccount>,
    ) -> Result<AuthenticatedAccount, AccountErrorCode> {
        let account = authenticated.ok_or(AccountErrorCode::Unauthenticated)?;
        if account.namespace != AuthenticatedNamespace::WipeableTest {
            return Err(AccountErrorCode::ProductionNamespaceForbidden);
        }
        Ok(account)
    }

    fn apply_mutation(
        &self,
        aggregate: &mut AccountAggregate,
        frame: &CharacterMutationFrame,
        foreign_character: bool,
    ) -> CharacterMutationResult {
        if let Some(cached) = aggregate
            .mutations
            .iter()
            .find(|cached| cached.mutation_id == frame.mutation_id)
        {
            return if cached.payload_hash == frame.payload_hash {
                cached.result.clone()
            } else {
                rejected_with_snapshot(
                    frame.mutation_id,
                    AccountErrorCode::IdempotencyConflict,
                    aggregate,
                )
            };
        }
        if aggregate.mutations.len() == MAX_ACCOUNT_MUTATION_RESULTS {
            return rejected_with_snapshot(
                frame.mutation_id,
                AccountErrorCode::RateLimited,
                aggregate,
            );
        }
        let result = if frame.expected_account_version != aggregate.version {
            rejected_with_snapshot(
                frame.mutation_id,
                AccountErrorCode::StateVersionMismatch,
                aggregate,
            )
        } else if foreign_character {
            rejected_with_snapshot(
                frame.mutation_id,
                AccountErrorCode::CharacterNotOwned,
                aggregate,
            )
        } else {
            match &frame.payload {
                CharacterMutationPayload::Create { class_id } => {
                    self.create_character(aggregate, frame.mutation_id, class_id)
                }
                CharacterMutationPayload::Select { character_id } => {
                    self.select_character(aggregate, frame.mutation_id, *character_id)
                }
            }
        };
        aggregate.mutations.push_back(CachedMutation {
            mutation_id: frame.mutation_id,
            payload_hash: frame.payload_hash,
            result: result.clone(),
        });
        if let Some(code) = result.error {
            self.events.record(IdentityEvent::MutationRejected { code });
        }
        result
    }

    fn create_character(
        &self,
        aggregate: &mut AccountAggregate,
        mutation_id: [u8; MUTATION_ID_BYTES],
        class_id: &WireText<96>,
    ) -> CharacterMutationResult {
        if class_id.as_str() != GRAVE_ARBALIST_CLASS_ID {
            return rejected_with_snapshot(mutation_id, AccountErrorCode::ClassDisabled, aggregate);
        }
        if aggregate.characters.len() >= usize::from(CORE_CHARACTER_SLOT_CAPACITY) {
            return rejected_with_snapshot(
                mutation_id,
                AccountErrorCode::CharacterSlotFull,
                aggregate,
            );
        }
        let character_id = self.id_generator.next_id();
        if all_zero(&character_id)
            || aggregate
                .characters
                .iter()
                .any(|character| character.id == character_id)
        {
            return rejected_with_snapshot(
                mutation_id,
                AccountErrorCode::ServiceUnavailable,
                aggregate,
            );
        }
        let roster_ordinal =
            u8::try_from(aggregate.characters.len() + 1).expect("Core character capacity fits u8");
        aggregate.characters.push(CharacterRecord {
            id: character_id,
            roster_ordinal,
            class_id: class_id.clone(),
            level: 1,
            oath_id: None,
            life_state: CharacterLifeState::Living,
            security_state: CharacterSecurityState::SafeCharacterSelect,
            state_version: 1,
        });
        aggregate.version += 1;
        self.events
            .record(IdentityEvent::CharacterCreated { roster_ordinal });
        accepted(mutation_id, aggregate)
    }

    fn select_character(
        &self,
        aggregate: &mut AccountAggregate,
        mutation_id: [u8; MUTATION_ID_BYTES],
        character_id: [u8; CHARACTER_ID_BYTES],
    ) -> CharacterMutationResult {
        let Some(character) = aggregate
            .characters
            .iter()
            .find(|character| character.id == character_id)
        else {
            return rejected_with_snapshot(
                mutation_id,
                AccountErrorCode::CharacterNotFound,
                aggregate,
            );
        };
        if character.life_state != CharacterLifeState::Living {
            return rejected_with_snapshot(mutation_id, AccountErrorCode::CharacterDead, aggregate);
        }
        let roster_ordinal = character.roster_ordinal;
        aggregate.selected_character_id = Some(character_id);
        aggregate.version += 1;
        self.events
            .record(IdentityEvent::CharacterSelected { roster_ordinal });
        accepted(mutation_id, aggregate)
    }
}

fn accepted(
    mutation_id: [u8; MUTATION_ID_BYTES],
    aggregate: &AccountAggregate,
) -> CharacterMutationResult {
    CharacterMutationResult {
        mutation_id,
        accepted: true,
        error: None,
        snapshot: Some(aggregate.snapshot()),
    }
}

fn rejected_with_snapshot(
    mutation_id: [u8; MUTATION_ID_BYTES],
    code: AccountErrorCode,
    aggregate: &AccountAggregate,
) -> CharacterMutationResult {
    CharacterMutationResult {
        mutation_id,
        accepted: false,
        error: Some(code),
        snapshot: Some(aggregate.snapshot()),
    }
}

fn result_without_snapshot(
    mutation_id: [u8; MUTATION_ID_BYTES],
    code: AccountErrorCode,
) -> CharacterMutationResult {
    CharacterMutationResult {
        mutation_id,
        accepted: false,
        error: Some(code),
        snapshot: None,
    }
}

const fn all_zero<const N: usize>(bytes: &[u8; N]) -> bool {
    let mut index = 0;
    while index < N {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU8, Ordering};

    use protocol::{AccountBootstrapRequest, AccountMessageValidationError};

    use super::*;

    #[derive(Debug, Clone, Copy)]
    struct FixedClock(u64);

    impl IdentityClock for FixedClock {
        fn unix_millis(&self) -> u64 {
            self.0
        }
    }

    #[derive(Debug, Default)]
    struct SequentialIds(AtomicU8);

    impl CharacterIdGenerator for SequentialIds {
        fn next_id(&self) -> [u8; CHARACTER_ID_BYTES] {
            [self.0.fetch_add(1, Ordering::Relaxed) + 1; CHARACTER_ID_BYTES]
        }
    }

    fn manifest() -> ManifestHash {
        ManifestHash::new("a".repeat(64)).unwrap()
    }

    fn service()
    -> IdentityService<InMemoryAccountRepository, FixedClock, SequentialIds, NoopIdentityEventSink>
    {
        IdentityService::new(
            InMemoryAccountRepository::default(),
            FixedClock(10_000),
            SequentialIds::default(),
            NoopIdentityEventSink,
            manifest(),
        )
    }

    fn account(value: u8) -> AuthenticatedAccount {
        AuthenticatedAccount {
            account_id: AccountId::new([value; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        }
    }

    fn bootstrap_frame() -> AccountBootstrapFrame {
        AccountBootstrapFrame {
            sequence: 1,
            request: AccountBootstrapRequest::Bootstrap,
            content_manifest_hash: manifest(),
        }
    }

    fn mutation(id: u8, version: u64, payload: CharacterMutationPayload) -> CharacterMutationFrame {
        CharacterMutationFrame {
            mutation_id: [id; MUTATION_ID_BYTES],
            expected_account_version: version,
            payload_hash: payload.canonical_hash(),
            issued_at_unix_millis: 9_000,
            payload,
        }
    }

    fn create(id: u8, version: u64) -> CharacterMutationFrame {
        mutation(
            id,
            version,
            CharacterMutationPayload::Create {
                class_id: WireText::new(GRAVE_ARBALIST_CLASS_ID).unwrap(),
            },
        )
    }

    fn snapshot(result: &AccountBootstrapResult) -> &AccountSnapshot {
        let AccountBootstrapResult::Snapshot(snapshot) = result else {
            panic!("expected snapshot")
        };
        snapshot
    }

    #[tokio::test]
    async fn bootstrap_is_wipeable_isolated_and_privacy_safe() {
        let first_process = service();
        let created = first_process.mutate(Some(account(1)), &create(1, 1)).await;
        assert!(created.accepted);
        assert_eq!(
            snapshot(
                &first_process
                    .bootstrap(Some(account(1)), &bootstrap_frame())
                    .await,
            )
            .characters
            .len(),
            1
        );
        assert!(
            snapshot(
                &first_process
                    .bootstrap(Some(account(2)), &bootstrap_frame())
                    .await,
            )
            .characters
            .is_empty()
        );
        let restarted_process = service();
        assert!(
            snapshot(
                &restarted_process
                    .bootstrap(Some(account(1)), &bootstrap_frame())
                    .await,
            )
            .characters
            .is_empty()
        );
        assert_eq!(
            format!("{:?}", account(1).account_id),
            "AccountId(<redacted>)"
        );
    }

    #[tokio::test]
    async fn create_has_exact_defaults_two_slots_and_deterministic_ids() {
        let service = service();
        let first = service.mutate(Some(account(1)), &create(1, 1)).await;
        let first_snapshot = first.snapshot.as_ref().unwrap();
        assert_eq!(first_snapshot.account_version, 2);
        assert_eq!(first_snapshot.characters[0].character_id, [1; 16]);
        assert_eq!(first_snapshot.characters[0].roster_ordinal, 1);
        assert_eq!(first_snapshot.characters[0].level, 1);
        assert_eq!(first_snapshot.characters[0].oath_id, None);
        assert_eq!(
            first_snapshot.characters[0].life_state,
            CharacterLifeState::Living
        );
        assert_eq!(
            first_snapshot.characters[0].security_state,
            CharacterSecurityState::SafeCharacterSelect
        );
        assert!(
            service
                .mutate(Some(account(1)), &create(2, 2))
                .await
                .accepted
        );
        let full = service.mutate(Some(account(1)), &create(3, 3)).await;
        assert_eq!(full.error, Some(AccountErrorCode::CharacterSlotFull));
        assert_eq!(full.snapshot.unwrap().account_version, 3);
    }

    #[tokio::test]
    async fn create_and_select_are_versioned_retry_safe_and_account_bound() {
        let service = service();
        let create_frame = create(1, 1);
        let first = service.mutate(Some(account(1)), &create_frame).await;
        let repeated = service.mutate(Some(account(1)), &create_frame).await;
        assert_eq!(first, repeated);
        assert_eq!(first.snapshot.as_ref().unwrap().characters.len(), 1);

        let mut conflicting = create_frame.clone();
        conflicting.payload = CharacterMutationPayload::Create {
            class_id: WireText::new("class.veil_witch").unwrap(),
        };
        conflicting.payload_hash = conflicting.payload.canonical_hash();
        assert_eq!(
            service.mutate(Some(account(1)), &conflicting).await.error,
            Some(AccountErrorCode::IdempotencyConflict)
        );

        let stale = create(2, 1);
        let stale_result = service.mutate(Some(account(1)), &stale).await;
        assert_eq!(
            stale_result.error,
            Some(AccountErrorCode::StateVersionMismatch)
        );
        assert_eq!(stale_result.snapshot.unwrap().account_version, 2);

        let character_id = first.snapshot.unwrap().characters[0].character_id;
        let forged = mutation(3, 1, CharacterMutationPayload::Select { character_id });
        assert_eq!(
            service.mutate(Some(account(2)), &forged).await.error,
            Some(AccountErrorCode::CharacterNotOwned)
        );
        let select = mutation(4, 2, CharacterMutationPayload::Select { character_id });
        let selected = service.mutate(Some(account(1)), &select).await;
        assert!(selected.accepted);
        assert_eq!(
            selected.snapshot.unwrap().selected_character_id,
            Some(character_id)
        );
    }

    #[tokio::test]
    async fn malformed_hash_time_class_and_namespace_fail_closed() {
        let service = service();
        let mut bad_hash = create(1, 1);
        bad_hash.payload_hash = [9; 32];
        assert_eq!(
            service.mutate(Some(account(1)), &bad_hash).await.error,
            Some(AccountErrorCode::PayloadHashMismatch)
        );
        let mut future = create(2, 1);
        future.issued_at_unix_millis = 10_001;
        assert_eq!(
            service.mutate(Some(account(1)), &future).await.error,
            Some(AccountErrorCode::IssuedAtInvalid)
        );
        let disabled = mutation(
            3,
            1,
            CharacterMutationPayload::Create {
                class_id: WireText::new("class.ashen_vanguard").unwrap(),
            },
        );
        assert_eq!(
            service.mutate(Some(account(1)), &disabled).await.error,
            Some(AccountErrorCode::ClassDisabled)
        );
        let production = AuthenticatedAccount {
            account_id: account(1).account_id,
            namespace: AuthenticatedNamespace::Production,
        };
        assert_eq!(
            service.mutate(Some(production), &create(4, 1)).await.error,
            Some(AccountErrorCode::ProductionNamespaceForbidden)
        );
        assert_eq!(
            service.mutate(None, &create(5, 1)).await.error,
            Some(AccountErrorCode::Unauthenticated)
        );
    }

    #[tokio::test]
    async fn concurrent_stale_creates_commit_exactly_once() {
        let service = Arc::new(service());
        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let mut workers = Vec::new();
        for mutation_id in [1, 2] {
            let service = Arc::clone(&service);
            let barrier = Arc::clone(&barrier);
            workers.push(tokio::spawn(async move {
                barrier.wait().await;
                service
                    .mutate(Some(account(1)), &create(mutation_id, 1))
                    .await
            }));
        }
        barrier.wait().await;
        let mut results = Vec::with_capacity(workers.len());
        for worker in workers {
            results.push(worker.await.unwrap());
        }
        assert_eq!(results.iter().filter(|result| result.accepted).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| result.error == Some(AccountErrorCode::StateVersionMismatch))
                .count(),
            1
        );
        assert_eq!(
            snapshot(
                &service
                    .bootstrap(Some(account(1)), &bootstrap_frame())
                    .await,
            )
            .characters
            .len(),
            1
        );
    }

    #[tokio::test]
    async fn mutation_ledger_is_bounded_without_eviction() {
        let service = service();
        let created = service.mutate(Some(account(1)), &create(1, 1)).await;
        let character_id = created.snapshot.unwrap().characters[0].character_id;
        let mut version = 2;
        for mutation_id in 2..=u8::try_from(MAX_ACCOUNT_MUTATION_RESULTS).unwrap() {
            let result = service
                .mutate(
                    Some(account(1)),
                    &mutation(
                        mutation_id,
                        version,
                        CharacterMutationPayload::Select { character_id },
                    ),
                )
                .await;
            assert!(result.accepted);
            version += 1;
        }
        let overflow = service
            .mutate(
                Some(account(1)),
                &mutation(
                    250,
                    version,
                    CharacterMutationPayload::Select { character_id },
                ),
            )
            .await;
        assert_eq!(overflow.error, Some(AccountErrorCode::RateLimited));
        let repeated_first = service.mutate(Some(account(1)), &create(1, 1)).await;
        assert!(repeated_first.accepted);
        assert_eq!(repeated_first.snapshot.unwrap().characters.len(), 1);
    }

    #[test]
    fn protocol_result_validation_requires_snapshot_on_success() {
        let result = CharacterMutationResult {
            mutation_id: [1; 16],
            accepted: true,
            error: None,
            snapshot: None,
        };
        assert_eq!(
            result.validate(),
            Err(AccountMessageValidationError::MutationResultMismatch)
        );
    }
}
