# Data model

Pratica schema version 1 stores protocol mechanics in private SQLite. The
schema deliberately does not model clauses, headings, obligations, resources,
entities, fields, or other product-contract semantics. Contract terms are one
bounded UTF-8 Markdown blob plus an exact digest.

## Durable records

- `stewards` and their immutable versions retain scope identity, represented
  party, charter, source-catalog snapshot, basis identity/digest, and requester
  policy.
- `integrations` retain entrant, title, optional context snapshot, lifecycle,
  and creation identity.
- `tracks` bind one integration to one exact steward version and fixed bilateral
  party roster. Retirement is retained state.
- `negotiations` retain lifecycle and optional predecessor agreement.
- `offers` retain unique identity, negotiation-local sequence, author party,
  exact Markdown bytes, SHA-256 digest, predecessor head, and event identity.
- `assents` retain party, exact offer, current/withdrawn/stale state, and event
  provenance. They do not attach to a digest alone.
- `agreements` retain one immutable seal over the negotiation, exact head offer,
  complete party assent set, and exact steward basis.
- `attempts` retain kind, immutable Nucleus request/job identity, selected
  domain head and basis, predecessor attempt, tool delivery, runtime state, and
  domain-commit outcome.
- `integration_reviews` retain exact reviewed agreement references and an
  immutable advisory Markdown result.
- `conformance_reviews` retain one sealed agreement, one frozen candidate basis,
  attempt identity, and immutable review result.
- append-only events retain ordered protocol history. Current status and reports
  are projections over these durable records.

Concrete table names may combine closely related records, but the invariants
below are part of schema-one compatibility.

## Terms identity

Pratica bounds and validates UTF-8 before opening a write transaction. Stored
bytes are not normalized. The digest is checked on every exact read and by
doctor, but the offer identity—not the digest—is the assent target. Two offers
with identical bytes therefore remain two revisions with separate assent.

Every proposal is a complete snapshot. Omission means absence in the new terms,
not inheritance from the previous offer. Diffing or interpreting two Markdown
documents belongs to the caller or an advisory review, not storage admission.

## Current head and assent

Each negotiation has at most one current offer. Proposal commits only while its
expected base equals that head. In the same transaction Pratica advances the
head, records author assent, and marks every other current assent stale.

Assent commits only when the named offer remains head and the acting party is in
the fixed roster. Withdrawal commits only against that same current head. A
historical assent stays inspectable but cannot satisfy the current seal.

## Agreement seal

The seal transaction requires:

1. an open negotiation and active track;
2. the selected offer is still its head;
3. every fixed required party has unwithdrawn assent to that exact offer;
4. the steward version and recorded source/basis digests still equal the
   negotiation guards; and
5. no prior agreement exists for that negotiation.

The agreement is inserted last and is immutable. Triggers and transaction
ordering reject later update/delete of sealed history and orphan child rows.
Amendment creates a new negotiation with a predecessor reference.

## Basis staleness

Negotiation staleness and basis staleness are independent:

- a newer offer stales other-party assent inside a negotiation;
- a newer or changed steward/candidate basis affects present applicability.

Historical agreements retain the exact basis on which they were sealed.
Pratica never rewrites one merely because a target system later changed.
Verification can report that current applicability is unproven; conformance can
record evidence against an explicitly supplied successor basis.

## Nucleus attempts and idempotency

Before admission Pratica durably records the exact typed request and job ID.
Ambiguous admission can repeat only those byte-equivalent request bytes.
Accepted managed-tool calls are keyed by immutable attempt and call identity.
Identical redelivery returns the bound result; conflicting redelivery fails.

The selected head and basis are rechecked in the same transaction as the domain
event. A stale response records no offer, assent, review, or agreement.
Nucleus terminal completion without the required committed tool result is a
failed Pratica attempt. Conversely, a committed domain event remains success if
the harness later fails while receiving or finishing after its result.

## Initialization, integrity, and migration

`init` is the only command that creates schema one. New database bytes are mode
0600 under a mode-0700 state directory before SQLite opens them. Ordinary
commands refuse absent, foreign, incomplete, newer, or older schemas and never
migrate implicitly.

Doctor checks the exact schema object set, `PRAGMA user_version`, foreign keys,
SQLite integrity, event/order seals, stored content digests, agreement
requirements, attempt correlations, and private permissions. Any future schema
change requires quiescence, a retained backup including WAL/SHM state, an
explicit migration, an old-state fixture, and database-aware rollback.
