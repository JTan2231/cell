# CLI contract

Pratica commands operate on one explicitly selected schema-one database. The
default is `~/Library/Application Support/Pratica/pratica.db`; `--database`
takes precedence over `PRATICA_DATABASE`. Relative database paths are refused.
Ordinary commands never initialize or migrate a missing database.

All commands accept global `--json`. Human output is intended for terminals;
JSON uses the stable envelope described below.

Terms and context arguments name regular non-symbolic UTF-8 files. They are
bounded before allocation and retained only where the command contract says so.
Standard input is not inferred from a missing path.

## Storage and readiness

```sh
pratica [--database ABSOLUTE_PATH] init
pratica [--database ABSOLUTE_PATH] doctor
```

`init` exclusively creates an empty schema-one database and private parent
directory; it refuses an existing target. `doctor` checks schema identity,
foreign keys, integrity, immutable-event seals, exact offer digests, private
permissions, and strict Nucleus/toolset readiness without changing negotiation
state.

## Steward scopes

```sh
pratica steward register MANIFEST
pratica steward list
pratica steward show SCOPE [--version VERSION]
```

Registration validates a complete versioned steward manifest, freezes its
charter, source catalog and basis, and rejects conflicting reuse of the same
scope/version. An identical replay is idempotent. `list` and `show` read only
registered snapshots; omitting `--version` selects the latest registered
positive numeric `u32` version, not a mutable external file.

The TOML manifest uses `schema_version = 1`, string `scope`, positive numeric
`version`, string `party`, string `title`, and `charter_markdown`. Each
`[[sources]]` table supplies string `id`, string `kind`, `path`, and optional
string `revision`. Relative paths resolve against the manifest. Exact source
bytes are frozen on registration. Duplicate IDs, symbolic files, non-UTF-8 or
control/binary content, sensitive filenames/extensions, files over 4 MiB, and
catalogs over 32 MiB are refused. The contract body itself remains Markdown in
offer files, not fields in the manifest.

## Integrations and tracks

```sh
pratica integration open --entrant ENTRANT --title TITLE [--context FILE]
pratica integration status INTEGRATION
pratica integration review INTEGRATION
pratica integration report INTEGRATION

pratica track open INTEGRATION \
  --steward SCOPE [--steward-version VERSION] --terms FILE
pratica track retire TRACK --reason TEXT
```

Opening an integration records its fixed entrant identity and optional bounded
context Markdown. Opening a track selects one exact registered steward version,
fixes the entrant/steward roster, stores the first complete offer, and makes its
author's assent current.

`status` is a mechanical summary. `report` is a read-time rendering of durable
state. `review` creates one composition-review attempt against exact current
agreement references and succeeds only when an accepted review is committed.
Retirement requires a reason, preserves all history, and removes the track from
current coverage without cancelling or deleting records.

## Negotiation

```sh
pratica negotiation show NEGOTIATION
pratica negotiation history NEGOTIATION
pratica negotiation propose NEGOTIATION --base OFFER --terms FILE
pratica negotiation assent NEGOTIATION --offer OFFER
pratica negotiation withdraw NEGOTIATION --offer OFFER
pratica negotiation cancel NEGOTIATION --reason TEXT
pratica steward respond NEGOTIATION
```

`propose` is an optimistic complete replacement: `--base` must be the current
offer at commit time. It never patches or merges prior Markdown. The submitting
party assents to the new offer and every other prior assent becomes stale.

`assent` agrees to the named unchanged current offer and may seal the agreement
when all required parties and basis guards are current. `withdraw` removes the
caller's assent only from the named current offer. A stale or foreign offer is
refused without a write. `cancel` closes only an open unsealed negotiation.

`steward respond` creates a Nucleus attempt against the frozen head and basis.
An accepted `assent` may seal, a `counterproposal` creates a new complete offer
authored by the steward, and `blocked` records the reason without manufacturing
terms or assent.

## Attempts

```sh
pratica attempt show ATTEMPT
pratica attempt retry ATTEMPT
```

`show` correlates Pratica domain state with the exact Nucleus job and distinguishes
admission, runtime terminality, accepted tool delivery, and domain commit. It
shows request/source/result digests, byte counts, and cited managed source
references without printing the persisted request or frozen source bodies.
`retry` is allowed only for a terminal completed, failed, cancelled, lost, or
timed-out attempt with no committed Pratica result. A completed attempt is
eligible only because Nucleus completion without the required accepted tool
submission is not Pratica success. Retry creates a successor attempt and job
ID; it never changes or reuses the prior Nucleus job. A blocked steward or
review response is a committed domain result, not a retryable runtime failure.

## Agreements

```sh
pratica agreement show AGREEMENT
pratica agreement export AGREEMENT [--output FILE]
pratica agreement verify AGREEMENT
pratica agreement amend AGREEMENT --terms FILE
```

`show` renders the immutable seal, exact party assents, offer digest, terms,
basis and predecessor/successor references. `export` writes the exact Markdown
terms to a caller-selected absent regular file or standard output; it does not
reformat them. `verify` checks stored identity, digest, seal, assent, and basis
guards, then re-reads only the recorded managed source paths and classifies
present basis applicability as fresh, stale, or unknown. It invokes no model,
adapter, crawl, or target mutation and cannot prove behavioral conformance. The
historical agreement and seal never change; the fresh/stale/unknown verification
observation is retained separately.

`amend` opens a successor negotiation with the same parties, the selected
agreement as predecessor, and one complete entrant-authored offer. It never
unseals or overwrites the predecessor.

## Conformance

```sh
pratica conformance review AGREEMENT --candidate-basis MANIFEST
pratica conformance show REVIEW
```

The review command freezes the supplied candidate basis, submits one bounded
Nucleus job, and commits exactly one immutable conformance review. `show` reads
that review and its agreement/candidate correlations. A conformance review is
evidence analysis only: it neither runs product tests nor implements, deploys,
releases, assents, or changes the sealed agreement.

## Failures and automation

Commands return nonzero for invalid input, missing/unsupported storage, stale
base or offer, conflicting identity reuse, unauthorized party action, retired
or closed state, basis drift, unavailable Nucleus, malformed tool calls,
terminal runtime failure, or missing domain success. Validation failures commit
no partial protocol event.

Callers should retain printed Pratica identities and pass them back explicitly.
They must not infer agreement from Nucleus completion, agent prose, identical
Markdown digests, or a prior assent to a now-stale offer.

Machine output uses a common version-one envelope. Success is
`{"schema_version":1,"ok":true,"data":{"type":"..."}}`; failure is
`{"schema_version":1,"ok":false,"error":{"code":"...","message":"..."}}`.
Record identifiers are opaque even when current examples use readable prefixes.
