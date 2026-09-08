# Krisis architecture

Krisis identifies a user decision in one completed root exchange. A positive
classification produces a Markdown document containing a generated summary
heading and the complete normalized user/assistant conversation through that
exchange. A negative classification produces no document. The model decides
whether a decision occurred; code checks the result structure and renders text.

## Source and classification

The Stop hook stores only session/turn correlation. Reconciliation discovers
missed completed root turns through Conversations. The observer freezes the full
normalized conversation prefix through the selected completed exchange. Exchanges
completed before the activation baseline are ineligible. Missing or unfinished
hook sources fail on their first processing error.

The shared document builder uses `krisis/decision-document/1` with the managed
`submit_decision` tool. Its only result fields are `is_decision` and `summary`.
No quoted authority, source span, or context/action/result fields are required.
The full prompt must fit 262144 UTF-8 bytes; Krisis fails instead of truncating
history. The summary must be a nonblank single line of at most 1000 Unicode
scalar values for a positive result, and null for a negative result. These are
structural checks. Decision meaning is governed by the classifier instructions.

See [source documents](source-documents.md) for rendering, source completeness,
request permissions, and immutable schema identities.

## Durable processing and delivery

One serial `observe process` activation first delivers the oldest pending
document whose observation has not failed. Otherwise, it processes one observation.
Each observation binds an exact Annals config path and persistent library ID.
Changing either value cannot redirect an existing observation or handoff.

The builder saves its frozen source, exact Nucleus request, job identity, and
accepted tool result in a private run directory before acknowledgement. An explicit retry
resumes the same run and request after uncertainty. An ambiguous admission or result never creates
a new attempt. After classification settles, one SQLite transaction records the
observation verdict and, for a positive result, the exact document and digest in
`decision_documents`. A crash between the saved result and this transaction is
recovered by reading the same run.

The producer key is derived from host, canonical thread, and selected turn
identity. Krisis calls Annals `inbox accept --producer krisis --key KEY FILE`.
Annals accepts accessible, nonblank UTF-8 text with normal file, size, integrity,
and storage checks. It imposes no decision layout or metadata schema. Annals
owns the accepted bytes and generates its own storage and processing metadata.
Its configured librarian agent interprets the document.

Krisis keeps the outbox body until it commits the matching Annals receipt. It
then clears that body and retains the key, digest, target, verdict, and receipt.
The private document run still contains the frozen source and result. Routine
status does not expose this text. Annals retries later processing under its own
inbox contract; Krisis does not redeliver under a new key after an inbox failure.

The first processing error is retained with the observation in one transaction.
Failed observations and their pending documents are excluded from automatic work.
A later explicit retry preserves failure history and existing request or document
identity. A retained failure is an observation outcome, not a worker health fault.
`health` reads durable worker timing and current lock ownership; it does not use
the failed-observation count. See [CLI](cli.md#worker-health) for state meanings.

## Compatibility and authority

The observer uses this document builder as its sole active production path.
`document build` and `document render` expose the same construction engine for
local use. They do not enqueue documents or advance observer coverage. Retired
`daily` and `review` commands cannot generate or deliver accounts.

Krisis schema 6 retains Decisions and schema-one account history. Migration
requires the old account outbox and in-flight observations/classification jobs
to be settled before the cutover. It does not silently discard or convert an
unfinished account. Old codecs and job decoders exist for retained history,
not new capture. The legacy lifecycle feed remains read-only.

`annals-api` owns exchange contract 2: acceptance receipts, library identity,
opaque cursors, and complete accepted-document events. It does not prescribe
document meaning. `decisions::api` owns hook and operational CLI types. Historical
`krisis-api` account and lifecycle types retain their original meaning.
