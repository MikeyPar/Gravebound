# GB-M02 shared native network gate runbook

## Purpose

Run four nonpersistent native clients in one shared authoritative combat arena and record honest M02 human evidence. This runbook is subordinate to the three design documents and `GB-M02-GATE`.

## Build and launch

1. Run `tools\dev.cmd m02-package` from a clean checkout.
2. Open `dist\Gravebound-M02-Playtest`.
3. Start `Start Server.cmd`. Wait until the server reports `127.0.0.1:50000` and `server-cert.der` exists.
4. Start `Start All Clients.cmd` once. It launches four distinct credentials within the authored eight-second participant-lock window to form one four-player arena. You may instead start all four numbered launchers individually, provided all four start within eight seconds of the first.
5. Verify all four windows show `CONNECTED — AUTHORITATIVE 30 HZ`, `fp.1.0.0`, and the nonpersistent/Recall-unavailable labels.
6. Verify each window shows four player characters and the same enemy layout before recording results. If not, stop all processes and repeat from step 3.

## Human procedure

Each tester independently:

1. Moves with WASD, aims with the mouse, and fires with left mouse.
2. Uses right mouse and Space at least once.
3. Calls out one shared enemy and verifies its health/death changes agree across all windows.
4. Avoids hostile projectiles, contributes to defeating the hostile set, and presses `E` near an eligible personal pickup. Each player has a separate owner-bound copy: another player's collection must not remove your visible copy, and your own copy must remain collectible.
5. Continues until `COMBAT TEST COMPLETE — AUTHORITY CONFIRMED` or authoritative death appears.
6. Reports first confusion, perceived input quality, readable/unreadable attacks, death cause if applicable, defects, and desired next action.

Reconnect check: close one client during combat, then relaunch the same numbered client before three seconds elapse. The same character must reattach and resume movement in the same world; it remains vulnerable while LinkLost. After the exact three-second deadline, automatic Recall and a later nonpersistent fresh run are expected rather than reattachment. Survivors continue in either case.

## Pass evidence

- Four distinct clients were concurrently connected to one server and saw one shared enemy/projectile world.
- Shared enemy health/death facts matched across recipients; player movement and personal pickups remained owner-bound.
- Each client produced an outcome with no crash, divergence, cross-owner control, or false client-authored death/Recall.
- Controls were playable for all four testers under the tested local conditions.
- P0/P1 defects are zero. Any P2 affecting completion keeps the gate pending until fixed and retested.

Record results in [`GB-M02-session-record-template.md`](GB-M02-session-record-template.md). Do not enter names, email addresses, account IDs, IP addresses, or raw auth tickets.
