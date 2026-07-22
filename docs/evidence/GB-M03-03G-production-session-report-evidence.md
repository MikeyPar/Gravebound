# GB-M03-03G production combat-session report evidence

**Scope:** Production-server measurement foundation; this evidence does not close the ordinary-route or 25-journey gate.

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md`: `QA-005` requires automated journey execution and `QA-101` requires the complete private loop without developer commands.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-WORLD-001`, `CONT-HUB-002`, and the fixed Core route require combat admission to occur through the normal Realm Gate and private-world composition.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03G` and the M03 exit gate require production-server, real-QUIC, restart, timing, cleanup, and 25-journey evidence.

## Corrected measurement boundary

`BoundCorePrivateLifeServer` previously returned a constant zero for `combat_sessions_admitted`, even when its persistent session owner had successfully installed a live microrealm. The production report now obtains this value from `CorePrivateLifeSessionReport`.

The session owner increments the counter only after a new microrealm driver, terminal owner, reward authorities, and binding have been installed successfully. Transport reconnect and observer reattachment do not increment it. Failed or duplicate binding attempts do not increment it. Shutdown captures the value before retiring session state and reports it alongside the existing zero-residue measurements.

## Focused verification

- `cargo test -p server_app --lib live_microrealm_survives_handoff_and_link_lost_until_exact_unbind`
- `cargo clippy -p server_app --lib -- -D warnings`
- `cargo fmt --all -- --check`
- `git diff --check`

The focused lifecycle test proves one successful danger admission remains one admission across transport handoff and `LinkLost`, and that exact unbind still reaches zero residue. The final production-server/PostgreSQL/real-QUIC harness must corroborate this report across the required 25 ordinary journeys before `GB-M03-03G` or parent `GB-M03-03` can close.
