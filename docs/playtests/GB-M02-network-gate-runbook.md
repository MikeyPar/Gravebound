# GB-M02 native network gate runbook

## Purpose

Run four nonpersistent native clients against one local authoritative server and record M02 human evidence. This runbook is subordinate to the three design authorities, `GB-M02-GATE`, and `SPEC-CONFLICT-003`.

## Build and launch

1. Run `./tools/dev.cmd m02-package` from a clean checkout.
2. Open `dist/Gravebound-M02-Playtest`.
3. Start `Start Server.cmd`. Do not continue until it reports the listen address and certificate path.
4. Start `Start Client 1.cmd` through `Start Client 4.cmd`. Each launcher uses a distinct opaque local credential.
5. Verify all four windows show `CONNECTED — AUTHORITATIVE 30 HZ`, `fp.1.0.0`, and nonpersistent/Recall-unavailable labels.

## Human procedure

Each tester independently:

1. Moves with WASD and aims/fires with the mouse.
2. Uses right mouse and Space at least once.
3. Avoids hostile projectiles, kills the complete visible hostile set, and presses `E` near an eligible personal pickup.
4. Continues until `COMBAT TEST COMPLETE — AUTHORITY CONFIRMED` or authoritative death appears.
5. Reports first confusion, perceived input quality, readable/unreadable attacks, death cause if applicable, defects, and desired next action.

At least one client should close and relaunch during combat with the same numbered launcher to exercise reconnect. The character remains vulnerable during `LinkLost`; testers must not be coached to expect local immunity or a client-decided outcome.

## Pass evidence

- Four distinct clients were connected concurrently to the same server process.
- Each client produced one terminal combat-test outcome with no crash, divergence, cross-session snapshot leakage, or false client-authored death/Recall.
- Controls were judged playable by all four under the tested local conditions.
- Every P0/P1 is zero. Any P2 affecting completion keeps the gate pending until fixed and retested.
- Record that sessions are concurrent isolated authorities, not shared party combat, until `SPEC-CONFLICT-003` is resolved.

Use [`GB-M02-session-record-template.md`](GB-M02-session-record-template.md). Do not enter names, email addresses, account IDs, IP addresses, or raw auth tickets.
