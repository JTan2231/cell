# Krisis architecture

Krisis is a scoped feed into Annals. It decides only whether an eligible user
message contains a decision and, when it does, constructs one or more immutable
accounts of what was settled and what was observed by capture time.

## Authority and eligibility

An eligible source is a completed default root interactive turn, active or
archived, whose completion is after the write-once Krisis activation baseline
and which contains at least one nonblank user message. There is no file-change,
materiality, effect, enactment, current-force, or relevance gate.

A decision is an attributable user transition from practical openness to an
explicit settlement constraining intended future behavior or state. Only an
exact span in the cited user message supplies authority. Assistant text may
resolve a referential acceptance or describe context, action, or result; it
cannot create a decision.

Every admitted user authority receives exactly one durable `decision` or
`no_decision` verdict. `no_decision` is negative coverage, not a quality claim.

## Source scopes

Krisis resolves and verifies normalized source through Conversations. Level
zero contains all current-turn user authorities, the nearest preceding
assistant message for each when present, and at most the final assistant
message. Level one, requested at most once, adds the newest preceding normalized
messages that fit. Both levels are bounded to 64 messages and 262,144 aggregate
UTF-8 bytes. Mandatory content is never truncated; an oversized scope fails.

The model sees opaque aliases, normalized role/text, and authority-turn
relationships. Occurrence time and precision remain host-derived. It does not
receive real host/thread/turn/item IDs, paths, file activity, commands,
tools, reasoning, approvals, raw Codex items, local execution, or web access.

## Classification

Nucleus executes `gpt-5.6-terra` with medium reasoning, workspace `none`, and
the single-tool set `krisis/decision-account-classification/1`. The only tool is
`submit_decision_account_classification`, with immutable input schema
`krisis.tool.submit-decision-account-classification.input.v1` and result schema
`krisis.tool.decision-account-classification.result.v1`.

The terminal result covers every authority and contains no confidence,
disposition, review, supersession, importance, truth, enactment, or current-force
field. Each decision account proposal contains an exact authority quote of at
most 500 bytes, a 1–1,000-byte statement, nullable 1–1,000-byte context/action/
result fields, and unique supporting aliases. At most 100 accounts may be
returned. Krisis validates the whole result and derives stable IDs from the real
host, authority item, and exact UTF-8 span outside the model boundary.

## Durable commit and delivery

Before classification, Krisis verifies Annals `decision-feed watermark` and
durably binds the observation to the exact config path and persistent library
ID. Before acknowledging a valid tool call, one transaction records complete
binary coverage, stable IDs and anchors, deterministic account Markdown and
SHA-256, that target identity in the delivery outbox, Nucleus job/call and exact
argument digests, the receipt, and the bounded accepted result. Identical replay
is idempotent; conflicting call arguments or target reuse fails closed.

Classification coverage may be complete while delivery is still pending; each
decision account remains pending end to end until Annals accepts it.
`observe process` always attempts the oldest pending account before classifying
new work. It invokes exactly:

```text
annals --config CONFIG --json inbox accept --producer krisis --key DECISION_ID FILE
```

The success envelope is `{ "ok": true, "data": RECEIPT }`. Krisis validates
contract version, dedicated library ID, producer, key, source digest, Annals job
identity, acceptance time, and `created` or `replayed` disposition before
committing the receipt. Annals outage retries exact delivery against the bound
config and library and never triggers reclassification. Exact owned temporary
handoff files are reused after uncertainty and scavenged only after a valid
receipt; cleanup failure remains visible and leaves the outbox pending.

The deterministic Markdown sections are `Decision`, `Authority`, `Context`,
`Action`, `Result`, and `Source`. Source is exact schema-version-1 JSON with the
decision ID, Unix occurrence second and precision, capture-rule version, and
authority host/thread/turn/item/span. An unobserved field renders as `Unknown.`

After acceptance Krisis discards the outbox body and new account prose and
non-authority support rows. It retains decision ID and digest, exact authority
anchor, binary coverage, source/job correlation, and the Annals receipt. Search,
retrieval, libraries, and later interpretation belong to Annals.

## Recovery and compatibility

Observation is serial. Ambiguous Nucleus admission resumes the same request and
job. Only a positively terminal failure permits a successor; a scope has one
initial attempt plus two successors. Level-one expansion is a new scope, not a
retry. A committed domain result remains authoritative despite later harness or
transport failure.

SQLite schema 4 is additive over Decisions history. Legacy classifier receipts,
candidates, reviews, digests, deliveries, and lifecycle events remain decodable.
Migration refuses unsettled `decisions-observe-*` correlations so the public
identity transition cannot strand an in-flight legacy job.
Krisis does not append new legacy lifecycle events. The read-only
`decisions.lifecycle.consume` command surface remains for existing consumers;
daily digest, review, email, and their schedules are retired.
