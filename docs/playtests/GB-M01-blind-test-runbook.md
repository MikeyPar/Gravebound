# GB-M01 blind playtest runbook

This is the executable human protocol for roadmap cohort `GB-TP02` and milestone gate `GB-M01-GATE`. It must be used with the canonical GDD, Content Production Specification, Development Roadmap, and `docs/milestones/GB-M01-completion-plan.md`; it does not replace them.

## Purpose and non-negotiable rules

- Test one immutable release executable and content bundle for the entire cohort.
- Recruit at least 10 complete testers who did not implement, tune, review, or previously test the features under evaluation.
- Obtain consent before enabling local telemetry. A declined or incomplete consent session may be observed only under the approved research policy and is excluded from metrics.
- Never collect a name, email, account/platform ID, IP address, handle, phone number, URL, or raw transcript in Gravebound artifacts.
- Give no verbal coaching beyond the in-game experience. Record confusion; do not resolve it unless safety or a hard blocker requires intervention.
- Ask what killed the tester before showing the detailed death trace.
- Record observed behavior separately from tester opinion.
- Never convert a missing answer, incomplete session, developer-tool run, or ineligible tester into a pass.

## Immutable test-pair preflight

Before recruitment, the test owner records:

1. Release executable `build_id` shown in the LocalLab HUD and emitted in telemetry.
2. Content bundle `fp.1.0.0` and its full package BLAKE3 hash from content validation.
3. SHA-256 of the distributed executable.
4. `GB-M01-09` passing performance report path and hash.
5. Date, Windows version, display resolution, and test owner pseudonym.
6. A clean local gate result: format, warnings-denied Clippy, all workspace tests, strict content validation, deterministic trace replay, release build, and release smoke.

If the executable or content bytes change, close the cohort, assign a new pair, and restart the ten-tester denominator. Do not combine results across pairs.

## Tester eligibility and IDs

Assign opaque values without an external identity mapping in this repository:

- tester: `tester-` followed by 16 lowercase hexadecimal characters;
- session: `session-` followed by 16 lowercase hexadecimal characters.

Mark `eligible_blind` only when all are true:

- consent is complete;
- the tester did not contribute to or previously test the evaluated features;
- the session used default time scale, no invulnerability, and no developer/debug intervention;
- the tester reached a terminal death or victory and completed every required question;
- the telemetry export is readable, ordered, and belongs to the immutable build/content pair.

Otherwise use the exact exclusion reason `excluded_feature_contributor`, `excluded_incomplete_consent`, `excluded_developer_tools`, `incomplete_session`, or `wrong_build_pair`. Excluded sessions remain useful qualitative evidence but never enter gate denominators.

## Launch procedure

Create a new output path for every session. Do not overwrite prior evidence.

```powershell
$env:GRAVEBOUND_TELEMETRY_CONSENT = '1'
$env:GRAVEBOUND_TELEMETRY_TESTER_ID = 'tester-<16 lowercase hex>'
$env:GRAVEBOUND_TELEMETRY_SESSION_ID = 'session-<16 lowercase hex>'
$env:GRAVEBOUND_TELEMETRY_COHORT = 'eligible_blind'
$env:GRAVEBOUND_TELEMETRY_GENRE_FAMILIARITY = 'new_to_both'
$env:GRAVEBOUND_TELEMETRY_OUTPUT = (Join-Path $PWD 'playtest-evidence\<session-id>.jsonl')
.\target\release\client_bevy.exe
```

Allowed familiarity values are `new_to_both`, `action_rpg_only`, `bullet_hell_only`, and `action_rpg_and_bullet_hell`. Ordinary sessions must not set an evidence scenario or enable developer controls.

Confirm before handing over control:

- the HUD build ID and `fp.1.0.0` match the cohort record;
- the game is at the first actionable LocalLab state;
- sound is audible at a comfortable level;
- keyboard and mouse work;
- no debug time scale or invulnerability state is active;
- the operator can observe without obstructing the display.

## Moderated session script

Read only this introduction:

> Please play this build as you naturally would. I will mostly stay quiet and take notes. You can stop at any time. When the run ends, I will ask a few questions.

During play, timestamp only observable facts in the session record:

1. First confusion: visible hesitation, repeated ineffective input, or explicit confusion.
2. First damage: whether the player noticed the hit and its direction/source.
3. First item: whether the player noticed, understood, and acted on the reward.
4. First death: immediate verbal/nonverbal reaction without prompting.
5. Restart action: whether Run Again was activated voluntarily and elapsed time from summary availability.

Do not describe controls, enemy rules, attack gaps, item choices, damage causes, or the restart action. If the tester asks for help, record the question. Answer only after the relevant observation is complete and mark the session as coached/ineligible when the help could affect a gate result.

## Death-cause protocol

Before expanding or explaining the detailed trace, ask in this order:

1. `What killed you?`
2. `Which attack or hazard was it?`
3. Present four choices only after the open response: the authoritative killer/pattern plus three plausible attacks actually present in the build.

Record the open response and selected killer/pattern first. Then reveal the authoritative death trace and record its `killer_id`, `pattern_id`, damage type, raw/final damage, and pre-hit health. `killer_correct` and `pattern_correct` are exact ID comparisons; researcher interpretation cannot override them. A gate success requires the tester to correctly identify what killed them before trace reveal.

For a victory without death, mark death-cause fields `not_observed`; the session can inform feel but does not satisfy the killer-understanding denominator unless the gate owner has a separate eligible death from that tester on the same pair.

## Post-run questions

Ask and record every item without leading language:

1. Movement feel, integer `1..5`.
2. Shooting feel, integer `1..5`.
3. Dodging feel, integer `1..5`.
4. Overall combat feel, integer `1..5`.
5. `What felt distinctive?`
6. `What would make you stop?`
7. `What do you want to do next?`
8. `Do you want another attempt?` as `yes` or `no`.

The research artifact stores only a researcher-authored, de-identified summary of each open answer, at most 280 bytes. Do not paste raw speech. Record whether the tester voluntarily restarted independently of their stated desire; the reroll gate requires both.

## Session closeout

1. Exit the client normally so telemetry publishes atomically.
2. Confirm the JSON Lines file exists and begins with `session_started`, contains the run events, and ends with `session_ended`.
3. Confirm every row has the expected tester/session/build/bundle values and `local_lab_no_account`.
4. Hash the telemetry export and completed session record with SHA-256.
5. File each defect with reproduction, player impact, evidence path, likely subsystem owner, and `P0`–`P3` severity.
6. Store evidence locally under access control; do not commit raw cohort evidence to the public repository.

Use [GB-M01-session-record-template.md](GB-M01-session-record-template.md) for the operator record.

## Gate calculation

Compute results only from complete `eligible_blind` records on the immutable pair:

- population: at least 10;
- killer understanding: at least 8 of the first 10 eligible testers correctly identify the lethal cause before trace;
- reroll desire: at least 7 of 10 both voluntarily restart and say they want another attempt;
- feel: median movement, shooting, dodging, and overall combat feel are each at least 4/5;
- no unresolved hostile-projectile, safe-zone, exit, grayscale, or center-screen obstruction defect;
- all automated, performance, determinism, content, and reliability evidence remains passing.

Publish raw eligible denominators, exclusions by reason, medians, hashes, failures, and a recorded `PASS` or `PENDING`. Any failed conjunct keeps `GB-M01-GATE` pending and authorizes only focused M01 tuning—not M02 feature expansion.
