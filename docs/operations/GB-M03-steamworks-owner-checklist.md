# GB-M03 Steamworks owner checklist

This checklist tracks the owner-controlled portion of `GB-M03-14`. It does not request, store, or validate confidential partner data, and it introduces no Steam runtime dependency.

Valve's current first-party references are:

- [Steamworks onboarding](https://partner.steamgames.com/doc/gettingstarted/onboarding?l=english)
- [Steamworks SDK overview](https://partner.steamgames.com/doc/sdk?language=english)
- [Uploading to Steam](https://partner.steamgames.com/doc/sdk/uploading?language=english)
- [Steam review process](https://partner.steamgames.com/doc/store/review_process?language=english)
- [Steam Playtest](https://partner.steamgames.com/doc/features/playtest?language=english)

Recheck those pages at execution time because platform requirements can change.

## Partner and legal owner actions

- [ ] Confirm the publishing person/entity and authority to distribute Gravebound.
- [ ] Complete the Steamworks partner identity and contact workflow.
- [ ] Execute the current required partner agreements through the Steamworks site.
- [ ] Complete bank, tax, and identity verification under the matching legal owner.
- [ ] Pay the product application fee through Steamworks.
- [ ] Configure at least two named partner users with least privilege; reserve legal/financial authority for the owner.
- [ ] Store credentials, MFA, recovery codes, legal documents, and financial records in owner-approved private systems, never this repository.

## Product and platform owner actions

- [ ] Create the base game application and record its non-secret App ID only after the owner approves publication of that identifier.
- [ ] Reserve and verify the customer-visible product name and supported Windows platform.
- [ ] Create a Steam Playtest child application only when the private cohort and server-capacity gates justify it.
- [ ] Prepare required store/library graphical assets and accurate product disclosures for later review.
- [ ] Define private beta/depot branches and access roles; do not expose a public branch by default.
- [ ] Upload a hashed Windows package through SteamPipe only after the standalone package passes release review.
- [ ] Submit store/build checklists with sufficient lead time for Valve review and changes.
- [ ] Record rollback instructions and retain the previous known-good depot build.

## M03 runtime boundary check

- [ ] No Steamworks SDK source, redistributable runtime, API binding, App ID file, partner credential, or depot script containing identifiers/secrets is added to the M03 game workspace.
- [ ] Standalone identity remains the only M03 test identity authority.
- [ ] Steam overlay, achievements, cloud saves, lobbies, inventory, authentication, commerce, and ownership checks remain disabled/unimplemented.
- [ ] Steam availability cannot be required for the M03 client to start or for the server to preserve durable terminal state.

## Redacted completion evidence

For each completed owner action, record only:

- action label;
- completion date in UTC;
- accountable role, not a personal legal identity;
- status (`complete`, `pending review`, or `blocked`);
- non-secret application identifier only if the owner approves it for repository publication; and
- a reference to the private evidence location, not the evidence itself.

Do **not** commit screenshots or text containing legal names, addresses, signatures, bank/tax details, payment information, credentials, MFA/recovery data, private email addresses, Steamworks session data, or confidential agreement terms.

## Current Next Step

The product owner completes the partner/legal/account actions in Steamworks and supplies a redacted completion record. Release engineering then confirms the shipped M03 package remains standalone and attaches only approved non-secret identifiers to `GB-M03-14`.
