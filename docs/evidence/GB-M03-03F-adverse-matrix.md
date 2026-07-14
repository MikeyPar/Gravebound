# GB-M03-03F adverse transition and recovery matrix

**Status:** Evidence map active; hosted real-QUIC/PostgreSQL execution remains the final transport gate.

## Authority

- `Gravebound_Production_GDD_v1_Canonical.md`: `SIM-013`, `DNG-030`, `TECH-010`-`015`, and `QA-005`-`007` require server-owned LinkLost timing, terminal precedence, idempotency, and fail-closed recovery.
- `Gravebound_Content_Production_Spec_v1.md`: the unpromoted Core world identity, fixed private-life route, exact arrivals, disabled branches, and independent content hashes remain unchanged.
- `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` requires restart/idempotency gates, 25 complete scripted journeys, and median login-to-control below 30 seconds.

The matrix is intentionally layered. Reliable QUIC proves framing and disposable route composition; focused lifecycle and PostgreSQL tests retain exact tick, transaction, and crash boundaries without replacing production authorities with a monolithic test double.

## Matrix

| Adverse case | Closest authoritative evidence | Required invariant |
|---|---|---|
| Lost committed response | `core_route_quic::reliable_quic_traverses_disposable_core_route_and_committed_extraction` | exact Hall mutation replay returns the committed snapshot and allocates no second lineage |
| Duplicate/stale request | `world_flow_coordinator::exact_replay_resequences_and_changed_binding_conflicts`; `core_world_transition::stale_request_sequences_and_results_cannot_mutate_the_projection` | exact retry is idempotent; changed binding and stale sequence fail closed |
| Transfer-boundary disconnect | `runtime::core_identity_real_quic_reconnects_and_server_restart_wipes`; `lifecycle::reconnect_preserves_advanced_authority_and_reports_resolved_route` | transport replacement cannot rewind authority or invent a destination |
| Reconnect at 89/90 ticks | `lifecycle::exact_link_lost_boundary_reconnects_at_89_and_recalls_at_90` | tick 89 may reattach; tick 90 resolves authoritative Recall before reconnect |
| Death on Recall boundary | `lifecycle::authoritative_death_wins_on_the_recall_boundary_tick` | authoritative death wins and cannot be reversed by Recall or reconnect |
| Extraction versus crash restore | `postgres_caldus_victory::crash_restore_between_request_and_receipt_supersedes_caldus_extraction`; `caldus_committed_receipt_supersedes_restore_and_transfers_once_to_hall_default` | pre-receipt restore wins; committed receipt wins exactly once afterward |
| Duplicate session | `lifecycle::duplicate_join_atomically_replaces_transport_and_preserves_authority` | one active transport remains and the replacement retains authoritative state |
| Content mismatch | real-QUIC rejection in `core_route_quic`; `world_flow_gate::well_shaped_payload_hash_and_revision_mismatches_are_typed` | mismatched independent hash returns `ContentMismatch` without allocation or mutation |
| Allocation/provider failure | `postgres_world_flow::concurrent_entry_has_one_lineage_and_provider_failure_rolls_back_every_row` | the composite danger root commits completely or leaves zero partial rows |
| Pool/process restart | `postgres_world_flow::danger_entry_commits_complete_root_and_replays_after_pool_restart`; `runtime::core_identity_real_quic_reconnects_and_server_restart_wipes` | PostgreSQL authority replays durably; deliberately in-memory identity wipes on process restart |
| Corrupt/foreign state | `postgres_world_flow::stale_foreign_and_corrupt_state_fail_closed_without_danger_allocation` | corrupt or foreign bindings allocate nothing and expose no route |
| Normal-route negative | `world_flow_gate::normal_selected_transfer_is_stage_disabled_without_a_transfer_identity`; normal runtime feature-flag assertion in `runtime::core_identity_real_quic_reconnects_and_server_restart_wipes` | normal Character Select and Realm Gate remain `StageDisabled`; the disposable feature is not advertised |

## Scripted route gate

`core_route_quic::reliable_quic_completes_25_scripted_core_journeys_below_login_budget` runs 25 serial journeys against the explicitly wipeable PostgreSQL namespace. Every journey performs:

1. real QUIC handshake and disposable feature negotiation;
2. Character Select to Hall with a deliberately discarded committed response and exact replay;
3. content-mismatched Realm Gate rejection, followed by exact valid danger entry;
4. production Caldus reward/progression victory commit and derived extraction identity;
5. committed extraction to `HallDefault` and exact extraction replay;
6. durable final-version assertion with no production inventory admission.

Each connection-through-authoritative-Hall sample must remain below 30 seconds; the sorted median, p95, and maximum are printed by the hosted guarded run. A failure in any iteration fails the mandatory PostgreSQL job.

## Scope boundary

This evidence does not enable production namespaces, normal Character Select `Play`, normal Realm Gate admission, seeded Bell branches, production inventory conversion, Core promotion, or affected Hall stations. Party/public allocation and party reconnect remain later-roadmap ownership.
