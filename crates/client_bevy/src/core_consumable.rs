//! Client-side intent and replay model for authoritative Core Belt consumables.
//!
//! The model never predicts custody. It binds Q/E intents to the latest server projection and
//! retains the exact mutation frame across transport loss until a matching durable result arrives.

use protocol::{
    CORE_CONSUMABLE_SCHEMA_VERSION, CoreConsumableResultCodeV1, CoreConsumableSlotV1,
    CoreConsumableStateV1, CoreConsumableUseFrameV1, CoreConsumableUsePayloadV1,
    CoreConsumableUseResultV1, CorePrivateRouteStateV1, ManifestHash,
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoreConsumableApplyOutcome {
    Accepted,
    Rejected(CoreConsumableResultCodeV1),
}

#[derive(Debug)]
pub(crate) struct CoreConsumableClientModel {
    expected_content_revision: ManifestHash,
    authority: Option<CoreConsumableStateV1>,
    pending: Option<CoreConsumableUseFrameV1>,
    last_result: Option<CoreConsumableResultCodeV1>,
}

impl CoreConsumableClientModel {
    pub(crate) const fn new(expected_content_revision: ManifestHash) -> Self {
        Self {
            expected_content_revision,
            authority: None,
            pending: None,
            last_result: None,
        }
    }

    pub(crate) fn observe_state(
        &mut self,
        state: CoreConsumableStateV1,
        selected_character_id: [u8; protocol::CHARACTER_ID_BYTES],
        route: &CorePrivateRouteStateV1,
    ) -> Result<(), CoreConsumableClientError> {
        state
            .validate()
            .map_err(|_| CoreConsumableClientError::InvalidAuthority)?;
        self.validate_authority(&state, selected_character_id, route)?;
        self.authority = Some(state);
        Ok(())
    }

    pub(crate) fn begin_use(
        &mut self,
        slot: CoreConsumableSlotV1,
        mutation_id: [u8; protocol::MUTATION_ID_BYTES],
        selected_character_id: [u8; protocol::CHARACTER_ID_BYTES],
        route: &CorePrivateRouteStateV1,
    ) -> Result<CoreConsumableUseFrameV1, CoreConsumableClientError> {
        if self.pending.is_some() {
            return Err(CoreConsumableClientError::MutationPending);
        }
        let authority = self
            .authority
            .as_ref()
            .ok_or(CoreConsumableClientError::AuthorityUnavailable)?;
        self.validate_authority(authority, selected_character_id, route)?;
        let payload = CoreConsumableUsePayloadV1 {
            character_id: selected_character_id,
            actor_generation: route.actor_generation,
            instance_lineage_id: route
                .instance_lineage_id
                .ok_or(CoreConsumableClientError::AuthorityUnavailable)?,
            content_revision: self.expected_content_revision.clone(),
            expected_inventory_version: authority.inventory_version,
            slot,
        };
        let frame = CoreConsumableUseFrameV1 {
            schema_version: CORE_CONSUMABLE_SCHEMA_VERSION,
            mutation_id,
            payload_hash: payload.canonical_hash(),
            payload,
        };
        frame
            .validate()
            .map_err(|_| CoreConsumableClientError::InvalidAuthority)?;
        self.pending = Some(frame.clone());
        Ok(frame)
    }

    pub(crate) fn apply_result(
        &mut self,
        result: CoreConsumableUseResultV1,
        selected_character_id: [u8; protocol::CHARACTER_ID_BYTES],
        route: &CorePrivateRouteStateV1,
    ) -> Result<CoreConsumableApplyOutcome, CoreConsumableClientError> {
        result
            .validate()
            .map_err(|_| CoreConsumableClientError::InvalidAuthority)?;
        let pending = self
            .pending
            .as_ref()
            .ok_or(CoreConsumableClientError::UnexpectedResult)?;
        if result.mutation_id != pending.mutation_id {
            return Err(CoreConsumableClientError::UnexpectedResult);
        }
        if let Some(state) = result.state.as_ref() {
            self.validate_authority(state, selected_character_id, route)?;
        }
        self.last_result = Some(result.code);
        if result.code == CoreConsumableResultCodeV1::ServiceUnavailable {
            return Ok(CoreConsumableApplyOutcome::Rejected(result.code));
        }
        self.pending = None;
        if let Some(state) = result.state {
            self.authority = Some(state);
        }
        Ok(if result.code == CoreConsumableResultCodeV1::Accepted {
            CoreConsumableApplyOutcome::Accepted
        } else {
            CoreConsumableApplyOutcome::Rejected(result.code)
        })
    }

    pub(crate) fn transport_lost(&mut self) {
        self.authority = None;
    }

    pub(crate) fn exact_retry(&self) -> Option<CoreConsumableUseFrameV1> {
        self.pending.clone()
    }

    pub(crate) fn cancel_for_new_route(&mut self) {
        self.authority = None;
        self.pending = None;
    }

    pub(crate) fn retain_for_route(
        &mut self,
        selected_character_id: Option<[u8; protocol::CHARACTER_ID_BYTES]>,
        route: Option<&CorePrivateRouteStateV1>,
    ) {
        let Some((character_id, route)) = selected_character_id.zip(route) else {
            self.cancel_for_new_route();
            return;
        };
        if self.pending.as_ref().is_some_and(|frame| {
            frame.payload.character_id != character_id
                || frame.payload.actor_generation != route.actor_generation
                || Some(frame.payload.instance_lineage_id) != route.instance_lineage_id
        }) {
            self.pending = None;
        }
        if self
            .authority
            .as_ref()
            .is_some_and(|state| self.validate_authority(state, character_id, route).is_err())
        {
            self.authority = None;
        }
    }

    pub(crate) fn belt_quantities(&self) -> Option<[u8; 2]> {
        self.authority.as_ref().map(|state| state.belt_quantities)
    }

    pub(crate) const fn last_result(&self) -> Option<CoreConsumableResultCodeV1> {
        self.last_result
    }

    pub(crate) const fn mutation_pending(&self) -> bool {
        self.pending.is_some()
    }

    fn validate_authority(
        &self,
        state: &CoreConsumableStateV1,
        selected_character_id: [u8; protocol::CHARACTER_ID_BYTES],
        route: &CorePrivateRouteStateV1,
    ) -> Result<(), CoreConsumableClientError> {
        if state.character_id != selected_character_id
            || state.actor_generation != route.actor_generation
            || Some(state.instance_lineage_id) != route.instance_lineage_id
            || state.content_revision != self.expected_content_revision
        {
            return Err(CoreConsumableClientError::InvalidAuthority);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum CoreConsumableClientError {
    #[error("authoritative Belt state is not available")]
    AuthorityUnavailable,
    #[error("an exact consumable mutation is already pending")]
    MutationPending,
    #[error("the server returned invalid consumable authority")]
    InvalidAuthority,
    #[error("the server returned a consumable result for an unknown mutation")]
    UnexpectedResult,
}
