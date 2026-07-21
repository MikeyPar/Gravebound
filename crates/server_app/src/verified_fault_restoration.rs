//! Server-planned terminal authority for verified private-route runtime faults.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-010` and `TECH-023`),
//! `Gravebound_Content_Production_Spec_v1.md` (Core danger-route authority), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-08` and the M03 restart gate).
//!
//! A fault signal contains no client-authored state. The active danger root supplies the only
//! restore destination, while the existing `PostgreSQL` crash-restore transaction atomically
//! restores the entry snapshot or reports the durable terminal mutation that already won.

use persistence::{DangerCrashRestoreRequest, derive_private_life_crash_mutation_id_v1};
use thiserror::Error;

use crate::{
    CorePrivateMicrorealmFaultKind, PreparedTerminal, TerminalBinding, TerminalCandidate,
    TerminalKind, TerminalValidationError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedVerifiedFaultRestoration {
    pub request: DangerCrashRestoreRequest,
    pub candidate: TerminalCandidate,
}

#[derive(Debug, Error)]
pub(crate) enum VerifiedFaultRestorationError {
    #[error("verified-fault restoration received invalid terminal authority")]
    InvalidAuthority,
    #[error("verified-fault restoration could not derive persistence authority")]
    Persistence(#[from] persistence::PersistenceError),
    #[error("verified-fault restoration could not build a terminal candidate: {0:?}")]
    Terminal(TerminalValidationError),
}

pub(crate) fn prepare_verified_fault_restoration(
    binding: TerminalBinding,
    expected_state_version: u64,
    observed_tick: u64,
    fault_kind: CorePrivateMicrorealmFaultKind,
) -> Result<PreparedVerifiedFaultRestoration, VerifiedFaultRestorationError> {
    if expected_state_version == 0
        || expected_state_version == u64::MAX
        || observed_tick == 0
        || matches!(
            fault_kind,
            CorePrivateMicrorealmFaultKind::TerminalAuthority
                | CorePrivateMicrorealmFaultKind::IndeterminateAuthority
        )
    {
        return Err(VerifiedFaultRestorationError::InvalidAuthority);
    }
    let mutation_id = derive_private_life_crash_mutation_id_v1(
        *binding.account_id(),
        *binding.character_id(),
        *binding.restore_point_id(),
    )?;
    let mut request = DangerCrashRestoreRequest {
        account_id: *binding.account_id(),
        character_id: *binding.character_id(),
        restore_point_id: *binding.restore_point_id(),
        mutation_id,
        request_hash: [0; 32],
    };
    request.request_hash = request.expected_request_hash();
    request.validate()?;

    let fault_code = [stable_fault_code(fault_kind)];
    let tick = observed_tick.to_be_bytes();
    let version = expected_state_version.to_be_bytes();
    let terminal_id = derived_identity(
        "gravebound.verified-fault-terminal-id.v1",
        &[
            &mutation_id,
            binding.lineage_id(),
            &tick,
            &version,
            &fault_code,
        ],
    );
    let server_plan_hash = derived_hash(
        "gravebound.verified-fault-server-plan.v1",
        &[
            &request.request_hash,
            binding.lineage_id(),
            &tick,
            &version,
            &fault_code,
        ],
    );
    let candidate = TerminalCandidate::from_server_plan(
        binding,
        terminal_id,
        mutation_id,
        request.request_hash,
        server_plan_hash,
        expected_state_version,
        observed_tick,
        TerminalKind::VerifiedServerFaultRestoration,
    )
    .map_err(VerifiedFaultRestorationError::Terminal)?;
    Ok(PreparedVerifiedFaultRestoration { request, candidate })
}

pub(crate) fn validate_fault_winner(
    prepared: &PreparedTerminal,
    restoration: &PreparedVerifiedFaultRestoration,
) -> Result<(), VerifiedFaultRestorationError> {
    if prepared.winner() != &restoration.candidate {
        return Err(VerifiedFaultRestorationError::InvalidAuthority);
    }
    Ok(())
}

const fn stable_fault_code(kind: CorePrivateMicrorealmFaultKind) -> u8 {
    match kind {
        CorePrivateMicrorealmFaultKind::RouteAuthority => 1,
        CorePrivateMicrorealmFaultKind::TickExhausted => 2,
        CorePrivateMicrorealmFaultKind::Simulation => 3,
        CorePrivateMicrorealmFaultKind::TerminalAuthority => 4,
        CorePrivateMicrorealmFaultKind::IndeterminateAuthority => 5,
    }
}

fn derived_identity(context: &str, parts: &[&[u8]]) -> [u8; 16] {
    let digest = derived_hash(context, parts);
    let mut identity = [0; 16];
    identity.copy_from_slice(&digest[..16]);
    identity
}

fn derived_hash(context: &str, parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_derive_key(context);
    for part in parts {
        hasher.update(&(part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_is_deterministic_and_binds_fault_boundary() {
        let binding = TerminalBinding::new([1; 16], [2; 16], [3; 16], [4; 16]).unwrap();
        let first = prepare_verified_fault_restoration(
            binding,
            7,
            41,
            CorePrivateMicrorealmFaultKind::Simulation,
        )
        .unwrap();
        let replay = prepare_verified_fault_restoration(
            binding,
            7,
            41,
            CorePrivateMicrorealmFaultKind::Simulation,
        )
        .unwrap();
        let changed = prepare_verified_fault_restoration(
            binding,
            7,
            41,
            CorePrivateMicrorealmFaultKind::RouteAuthority,
        )
        .unwrap();

        assert_eq!(first, replay);
        assert_eq!(first.request, changed.request);
        assert_ne!(first.candidate, changed.candidate);
        assert_eq!(
            first.candidate.kind(),
            TerminalKind::VerifiedServerFaultRestoration
        );
    }
}
