# Assess a Todo's current situation

A situation assessment describes the selected source material for one open
canonical `tN`. It maps accepted direction boundaries to findings, constraints,
dependencies, open items and jurisdiction. It records an assessment for later
design work.

```sh
/Users/joey/.local/bin/todo assess <TODO_ID>
/Users/joey/.local/bin/todo situation show <ASSESSMENT_ID>
```

## Frozen assessment basis

Todo freezes the current direction revision, attached concerns, working-note
cursor, accepted design if any, its own read projection, and a catalog of
relevant sources. Concrete documents receive stable source IDs. The liaison
can list, read, and search only that catalog through managed tools; it has no
shell, workspace, inherited environment, or web search.

Source reads return exact evidence references. The committed `aN` stores the
mapping from each cited source ID to its locator, frozen revision, and
observation time. Source text is untrusted evidence, not runtime instruction.

## Required result

One immutable dated assessment includes:

- subject identity and stable references;
- grounded current-state, constraint, dependency, and gap findings;
- coverage of every direction boundary;
- jurisdiction findings assigning every relevant party exactly one role of
  owner, participant, or consumer, with exactly one owner per jurisdiction;
- unresolved user choices, missing assessment inputs, and jurisdiction conflicts; and
- one disposition: `ready`, `needs_user_choice`, or `inconclusive`.

`ready` means the selected inputs support design reconciliation with no
unresolved items. `needs_user_choice` records a material value or authority
decision. `inconclusive` records missing assessment material. Runtime or tool
failure is infrastructure failure; do not relabel it as an inconclusive finding.

## Currentness and recovery

The historical `aN` is immutable and always inspectable. Changes to its frozen
bases make it stale without rewriting it. Any newer assessment for the same
umbrella makes every older one non-current. When facts or bases change, create
a new assessment rather than editing the old one.

The committed Todo assessment is the result. Nucleus job completion and model
prose do not create an assessment. Nucleus raw output may retain source content read by the
liaison, while Todo retains exact source references and frozen provenance.

## Rust callers

The provider crate exports `todo::api`: supported request and response
types, provider-owned envelope decoding, and an explicit-executable CLI client.
Use these types at imports and convert only to caller-local domain values.
The client performs the same operations under this contract and never adds
retry or authorization. See `todo/docs/rust-api.md`; the Rust structs and
enums define the interface without a separate declaration layer.
