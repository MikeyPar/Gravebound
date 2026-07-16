//! Concurrent `GB-M03-13` eligible-death transaction evidence.
//!
//! Authorities:
//! - `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-001`, `ECH-001`, `ECH-002`,
//!   and `ECH-003`;
//! - `Gravebound_Content_Production_Spec_v1.md`: `CONT-ECHO-009`;
//! - `Gravebound_Development_Roadmap_v1.md`: `GB-M03-02`, `GB-M03-06`, `GB-M03-13`,
//!   and the atomic qualifying-death exit gate.
//!
//! One account owns exactly one selected character, so two distinct successful deaths on one
//! account are not a reachable state. Same-account duplicate writers are already covered by the
//! durable-death repository gate. This target closes the remaining reachable race: two distinct
//! accounts commit eligible deaths concurrently, then concurrently replay the exact requests,
//! without cross-account Echo, death, outbox, signature, transaction, or lock contamination.

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use persistence::{
    DurableEchoOutcomeV1, PersistenceConfig, PostgresPersistence, StoredCommittedDeathResultV1,
    WIPEABLE_CORE_NAMESPACE,
};
use server_app::{
    DurableDeathExecutionService, PreparedDurableDeathCommit, SubmitResult, TerminalArbiter,
    TerminalCandidate, durable_death_terminal_candidate,
};
use sqlx::Row;
use tokio::sync::Barrier;

#[path = "support/death_measurement.rs"]
mod death_measurement;
#[path = "support/durable_death.rs"]
mod durable_death_fixture;

const CONCURRENCY_REPORT_PATH_ENV: &str = "GRAVEBOUND_DEATH_CONCURRENCY_REPORT_PATH";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AccountGraphCountsV1 {
    death_events: u32,
    death_summaries: u32,
    memorial_records: u32,
    destruction_entries: u32,
    death_mutation_results: u32,
    echo_records: u32,
    echo_transitions: u32,
    available_echoes: u32,
    dormant_echoes: u32,
    death_outbox_events: u32,
}

impl AccountGraphCountsV1 {
    const fn is_exact(self) -> bool {
        self.death_events == 1
            && self.death_summaries == 1
            && self.memorial_records == 1
            && self.destruction_entries == 2
            && self.death_mutation_results == 1
            && self.echo_records == 1
            && self.echo_transitions == 2
            && self.available_echoes == 1
            && self.dormant_echoes == 0
            && self.death_outbox_events == 3
    }

    fn checked_sum(self, other: Self) -> Self {
        Self {
            death_events: self.death_events.checked_add(other.death_events).unwrap(),
            death_summaries: self
                .death_summaries
                .checked_add(other.death_summaries)
                .unwrap(),
            memorial_records: self
                .memorial_records
                .checked_add(other.memorial_records)
                .unwrap(),
            destruction_entries: self
                .destruction_entries
                .checked_add(other.destruction_entries)
                .unwrap(),
            death_mutation_results: self
                .death_mutation_results
                .checked_add(other.death_mutation_results)
                .unwrap(),
            echo_records: self.echo_records.checked_add(other.echo_records).unwrap(),
            echo_transitions: self
                .echo_transitions
                .checked_add(other.echo_transitions)
                .unwrap(),
            available_echoes: self
                .available_echoes
                .checked_add(other.available_echoes)
                .unwrap(),
            dormant_echoes: self
                .dormant_echoes
                .checked_add(other.dormant_echoes)
                .unwrap(),
            death_outbox_events: self
                .death_outbox_events
                .checked_add(other.death_outbox_events)
                .unwrap(),
        }
    }
}

async fn disposable_database() -> PostgresPersistence {
    let config = PersistenceConfig::from_test_environment()
        .expect("TEST_DATABASE_URL must identify dedicated disposable PostgreSQL");
    let persistence = PostgresPersistence::connect(&config).await.unwrap();
    persistence.verify_disposable_test_database().await.unwrap();
    persistence.migrate().await.unwrap();
    persistence.reset_disposable_identity_data().await.unwrap();
    persistence
}

fn micros(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_micros())
        .unwrap_or(u64::MAX)
        .max(1)
}

async fn commit_after_barrier(
    persistence: &PostgresPersistence,
    death: &PreparedDurableDeathCommit,
    barrier: Arc<Barrier>,
) -> (TerminalCandidate, StoredCommittedDeathResultV1, bool) {
    let candidate = durable_death_terminal_candidate(death).unwrap();
    let mut arbiter = TerminalArbiter::new(candidate.binding());
    assert!(matches!(
        arbiter.submit(candidate.clone()),
        SubmitResult::Accepted { .. }
    ));
    let prepared = arbiter
        .prepare(death.request().plan.event.death_tick)
        .unwrap();
    barrier.wait().await;
    let committed = DurableDeathExecutionService::new(persistence.clone())
        .execute_prepared(&mut arbiter, &prepared, death)
        .await
        .unwrap();
    (
        candidate,
        committed.transaction.result().clone(),
        committed.transaction.is_replay(),
    )
}

async fn replay_after_barrier(
    persistence: &PostgresPersistence,
    candidate: &TerminalCandidate,
    death: &PreparedDurableDeathCommit,
    expected: &StoredCommittedDeathResultV1,
    barrier: Arc<Barrier>,
) -> bool {
    let mut arbiter = TerminalArbiter::new(candidate.binding());
    assert!(matches!(
        arbiter.submit(candidate.clone()),
        SubmitResult::Accepted { .. }
    ));
    let prepared = arbiter
        .prepare(death.request().plan.event.death_tick)
        .unwrap();
    barrier.wait().await;
    let replay = DurableDeathExecutionService::new(persistence.clone())
        .execute_prepared(&mut arbiter, &prepared, death)
        .await
        .unwrap();
    assert_eq!(replay.transaction.result(), expected);
    replay.transaction.is_replay()
}

async fn canonical_signature(
    persistence: &PostgresPersistence,
    scenario: &durable_death_fixture::DurableDeathScenarioV1,
) -> Vec<u8> {
    persistence
        .load_core_death_terminal_signature_v1(scenario.account_id, scenario.identity.character_id)
        .await
        .unwrap()
        .expect("committed death terminal signature")
        .canonical_bytes()
        .unwrap()
}

async fn account_graph_counts(
    persistence: &PostgresPersistence,
    account_id: [u8; 16],
) -> AccountGraphCountsV1 {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let row = sqlx::query(
        "SELECT \
         (SELECT count(*) FROM death_events \
          WHERE namespace_id=$1 AND account_id=$2) AS death_events,\
         (SELECT count(*) FROM death_summary_snapshots AS summary \
          JOIN death_events AS death USING (namespace_id,death_id) \
          WHERE summary.namespace_id=$1 AND death.account_id=$2) AS death_summaries,\
         (SELECT count(*) FROM memorial_records \
          WHERE namespace_id=$1 AND account_id=$2) AS memorial_records,\
         (SELECT count(*) FROM death_destruction_entries AS destroyed \
          JOIN death_events AS death USING (namespace_id,death_id) \
          WHERE destroyed.namespace_id=$1 AND death.account_id=$2) AS destruction_entries,\
         (SELECT count(*) FROM death_mutation_results \
          WHERE namespace_id=$1 AND account_id=$2) AS death_mutation_results,\
         (SELECT count(*) FROM echo_records \
          WHERE namespace_id=$1 AND account_id=$2) AS echo_records,\
         (SELECT count(*) FROM echo_state_transitions AS transition \
          JOIN echo_records AS echo USING (namespace_id,echo_id) \
          WHERE transition.namespace_id=$1 AND echo.account_id=$2) AS echo_transitions,\
         (SELECT count(*) FROM echo_records \
          WHERE namespace_id=$1 AND account_id=$2 AND state=1) AS available_echoes,\
         (SELECT count(*) FROM echo_records \
          WHERE namespace_id=$1 AND account_id=$2 AND state=0) AS dormant_echoes,\
         (SELECT count(*) FROM death_outbox_events AS outbox \
          JOIN death_events AS death USING (namespace_id,death_id) \
          WHERE outbox.namespace_id=$1 AND death.account_id=$2) AS death_outbox_events",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    let count = |column| u32::try_from(row.get::<i64, _>(column)).unwrap();
    AccountGraphCountsV1 {
        death_events: count("death_events"),
        death_summaries: count("death_summaries"),
        memorial_records: count("memorial_records"),
        destruction_entries: count("destruction_entries"),
        death_mutation_results: count("death_mutation_results"),
        echo_records: count("echo_records"),
        echo_transitions: count("echo_transitions"),
        available_echoes: count("available_echoes"),
        dormant_echoes: count("dormant_echoes"),
        death_outbox_events: count("death_outbox_events"),
    }
}

async fn cross_account_row_count(
    persistence: &PostgresPersistence,
    left: &durable_death_fixture::DurableDeathScenarioV1,
    right: &durable_death_fixture::DurableDeathScenarioV1,
) -> u32 {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT \
         (SELECT count(*) FROM death_events WHERE namespace_id=$1 AND account_id=$2 AND death_id=$3)+\
         (SELECT count(*) FROM memorial_records WHERE namespace_id=$1 AND account_id=$2 AND death_id=$3)+\
         (SELECT count(*) FROM echo_records WHERE namespace_id=$1 AND account_id=$2 AND echo_id=$4)+\
         (SELECT count(*) FROM death_events WHERE namespace_id=$1 AND account_id=$5 AND death_id=$6)+\
         (SELECT count(*) FROM memorial_records WHERE namespace_id=$1 AND account_id=$5 AND death_id=$6)+\
         (SELECT count(*) FROM echo_records WHERE namespace_id=$1 AND account_id=$5 AND echo_id=$7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(left.account_id.as_slice())
    .bind(right.identity.death_id.as_slice())
    .bind(right.identity.echo_id.as_slice())
    .bind(right.account_id.as_slice())
    .bind(left.identity.death_id.as_slice())
    .bind(left.identity.echo_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    u32::try_from(count).unwrap()
}

/// GDD `DTH-001`/`ECH-001`-`003`, Content `CONT-ECHO-009`, and Roadmap
/// `GB-M03-02`/`06`/`13` require two independent account locks to commit qualifying Echo
/// projectors without duplication or cross-account queue contamination.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
#[allow(
    clippy::too_many_lines,
    reason = "the complete concurrent commit, replay, isolation, and evidence sequence remains auditable"
)]
async fn concurrent_distinct_eligible_deaths_commit_and_replay_isolated_echoes() {
    let persistence = disposable_database().await;
    let left = durable_death_fixture::DurableDeathScenarioV1::primary_eligible();
    let right = durable_death_fixture::DurableDeathScenarioV1::parallel_eligible();
    assert_ne!(left.account_id, right.account_id);
    assert_ne!(left.identity.character_id, right.identity.character_id);
    assert_ne!(left.identity.death_id, right.identity.death_id);
    assert_ne!(left.identity.echo_id, right.identity.echo_id);

    durable_death_fixture::seed_danger_root_for(&persistence, &left).await;
    durable_death_fixture::seed_danger_root_for(&persistence, &right).await;
    let (left_death, right_death) = tokio::join!(
        Box::pin(durable_death_fixture::prepare_death_for(
            persistence.clone(),
            &left,
        )),
        Box::pin(durable_death_fixture::prepare_death_for(
            persistence.clone(),
            &right,
        )),
    );

    let commit_barrier = Arc::new(Barrier::new(2));
    let commit_started = Instant::now();
    let (left_commit, right_commit) = tokio::join!(
        Box::pin(commit_after_barrier(
            &persistence,
            &left_death,
            Arc::clone(&commit_barrier),
        )),
        Box::pin(commit_after_barrier(
            &persistence,
            &right_death,
            Arc::clone(&commit_barrier),
        )),
    );
    let concurrent_commit_micros = micros(commit_started.elapsed());
    let (left_candidate, left_result, left_replayed) = left_commit;
    let (right_candidate, right_result, right_replayed) = right_commit;
    assert!(!left_replayed && !right_replayed);
    assert_eq!(left_result.echo_outcome, DurableEchoOutcomeV1::Available);
    assert_eq!(right_result.echo_outcome, DurableEchoOutcomeV1::Available);

    let left_counts = account_graph_counts(&persistence, left.account_id).await;
    let right_counts = account_graph_counts(&persistence, right.account_id).await;
    assert!(left_counts.is_exact());
    assert!(right_counts.is_exact());
    let counts_before_replay = left_counts.checked_sum(right_counts);
    let left_signature_before = canonical_signature(&persistence, &left).await;
    let right_signature_before = canonical_signature(&persistence, &right).await;
    let cross_account_rows = cross_account_row_count(&persistence, &left, &right).await;
    assert_eq!(cross_account_rows, 0);

    let replay_barrier = Arc::new(Barrier::new(2));
    let replay_started = Instant::now();
    let (left_replay, right_replay) = tokio::join!(
        Box::pin(replay_after_barrier(
            &persistence,
            &left_candidate,
            &left_death,
            &left_result,
            Arc::clone(&replay_barrier),
        )),
        Box::pin(replay_after_barrier(
            &persistence,
            &right_candidate,
            &right_death,
            &right_result,
            Arc::clone(&replay_barrier),
        )),
    );
    let concurrent_replay_micros = micros(replay_started.elapsed());
    assert!(left_replay && right_replay);

    let left_signature_after = canonical_signature(&persistence, &left).await;
    let right_signature_after = canonical_signature(&persistence, &right).await;
    let counts_after_replay = account_graph_counts(&persistence, left.account_id)
        .await
        .checked_sum(account_graph_counts(&persistence, right.account_id).await);
    assert_eq!(counts_after_replay, counts_before_replay);
    let canonical_signatures_unchanged = left_signature_before == left_signature_after
        && right_signature_before == right_signature_after;
    assert!(canonical_signatures_unchanged);
    let final_database_residue =
        death_measurement::PostgresResidueSnapshotV1::capture(&persistence)
            .await
            .unwrap();
    assert!(final_database_residue.is_zero());

    let evidence = death_measurement::ConcurrentEligibleDeathsEvidenceV1::compile(
        death_measurement::ConcurrentEligibleDeathsInputV1 {
            account_count: 2,
            distinct_accounts_characters_deaths_and_echoes: true,
            exact_per_account_graphs: left_counts.is_exact() && right_counts.is_exact(),
            fresh_commits: 2,
            exact_replays: 2,
            death_events: counts_after_replay.death_events,
            death_summaries: counts_after_replay.death_summaries,
            memorial_records: counts_after_replay.memorial_records,
            destruction_entries: counts_after_replay.destruction_entries,
            death_mutation_results: counts_after_replay.death_mutation_results,
            echo_records: counts_after_replay.echo_records,
            echo_transitions: counts_after_replay.echo_transitions,
            available_echoes: counts_after_replay.available_echoes,
            dormant_echoes: counts_after_replay.dormant_echoes,
            death_outbox_events: counts_after_replay.death_outbox_events,
            cross_account_rows,
            canonical_signature_checks: 4,
            canonical_signatures_unchanged,
            concurrent_commit_micros,
            concurrent_replay_micros,
            final_database_residue,
        },
    )
    .unwrap();
    assert!(
        evidence.accepted,
        "concurrent eligible-death evidence failed: {evidence:#?}"
    );
    if let Some(path) = std::env::var_os(CONCURRENCY_REPORT_PATH_ENV) {
        evidence
            .write_json_atomically(&PathBuf::from(path))
            .unwrap();
    }
    println!(
        "GB_M03_13_CONCURRENT_ELIGIBLE_DEATHS_EVIDENCE={}",
        serde_json::to_string(&evidence).unwrap()
    );

    persistence.reset_disposable_identity_data().await.unwrap();
    persistence.close().await;
}
