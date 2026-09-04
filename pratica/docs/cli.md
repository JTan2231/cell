# CLI contract

Pratica commands operate on one explicitly selected schema-two database. The
default is `~/Library/Application Support/Pratica/pratica.db`; `--database`
takes precedence over `PRATICA_DATABASE`. Relative database paths are refused.
Ordinary commands never initialize or migrate a missing or older database.

All commands accept global `--json`. Human output is intended for terminals;
JSON uses the stable envelope described below.

Every top-level terms, context, or manifest argument accepts either a regular
non-symbolic UTF-8 file or the exact value `-` for standard input. Inputs are
bounded before allocation. A path is borrowed: Pratica never deletes or
changes a manifest, Markdown input, or referenced source file after reading it.
Standard input is never inferred from a missing path.

The five state-creating commands documented below accept an optional
`--request-key KEY`. Keys are global within the selected database, contain
1-256 visible ASCII characters without spaces, and identify one canonical
request for one operation. Reusing a key with the same canonical request
returns the originally bound durable identities; reusing it for another
operation or changed request fails without a domain write. The receipt and its
identity-establishing domain rows commit atomically. Replay guarantees those
recorded identities, not a byte-for-byte copy of the earlier command response;
any accompanying aggregate is a fresh projection of current state. A request
key is required when one of those commands reads its document from standard
input, so an ambiguous caller retry can be resolved without retaining a
transport file.

## Storage and readiness

```sh
pratica [--database ABSOLUTE_PATH] init
pratica [--database ABSOLUTE_PATH] migrate --backup ABSOLUTE_PATH
pratica [--database ABSOLUTE_PATH] doctor
```

`init` exclusively creates an empty schema-two database and private parent
directory; it refuses an existing target. `migrate` upgrades only schema one
to schema two. It requires a quiescent database with no active Pratica attempt
and creates a new mode-0600 SQLite backup at the selected absent absolute path
inside an already-private directory before changing the database. A current
schema-two database is a true no-op and does not inspect or create the supplied
backup path. Other source versions are refused.

Rollback across this migration restores the schema-one backup together with a
schema-one Pratica binary; a program-only rollback is unsafe. Ordinary commands
do not migrate implicitly. `doctor` checks schema identity,
foreign keys, integrity, immutable-event seals, exact offer digests, private
permissions, ingress receipt digests and JSON, and strict Nucleus/toolset
readiness without changing negotiation state.

## Steward scopes

```sh
pratica steward register MANIFEST [--source-root ABSOLUTE_DIRECTORY]
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

When `MANIFEST` is `-`, `--source-root` is required, must name an absolute real
directory, and becomes the base for every relative `[[sources]].path`.
Pratica never guesses the current directory. `--source-root` is refused with a
file-backed manifest, whose containing directory remains its base. Manifest
TOML formatting and comments are transport syntax and are not retained;
Pratica retains the normalized descriptor, charter, source metadata, exact
source bytes, and digests needed to continue without the manifest.

Registration output, `list`, and `show` are body-free projections. `list`
reports scope summaries; registration and `show` add the basis and source roster
with relative locators and per-source digests. None prints charter or source
bodies or canonical source-origin paths. Registration itself remains idempotent
by exact scope/version descriptor and basis.

## Integrations and tracks

```sh
pratica integration open --entrant ENTRANT --title TITLE \
  [--context FILE_OR_DASH] [--request-key KEY]
pratica integration list
pratica integration status INTEGRATION
pratica integration review INTEGRATION
pratica integration report INTEGRATION

pratica track open INTEGRATION \
  --steward SCOPE [--steward-version VERSION] \
  --terms FILE_OR_DASH [--request-key KEY]
pratica track retire TRACK --reason TEXT
```

Opening an integration records its fixed entrant identity and optional bounded
context Markdown. Opening a track selects one exact registered steward version,
fixes the entrant/steward roster, stores the first complete offer, and makes its
author's assent current.

`integration list` is a body-free newest-first discovery view containing
integration identity, entrant, title, optional context digest and byte count,
and creation time. It does not print context Markdown. `--request-key` is
optional for file-backed `integration open` and `track open`, and required when
their selected context or terms input is `-`.

`status` is a mechanical summary. `report` is a read-time rendering of durable
state. `review` creates one composition-review attempt against exact current
agreement references and succeeds only when an accepted review is committed.
Retirement requires a reason, preserves all history, and removes the track from
current coverage without cancelling or deleting records.

## Negotiation

```sh
pratica negotiation show NEGOTIATION
pratica negotiation history NEGOTIATION
pratica negotiation propose NEGOTIATION --base OFFER \
  --terms FILE_OR_DASH [--request-key KEY]
pratica negotiation assent NEGOTIATION --offer OFFER
pratica negotiation withdraw NEGOTIATION --offer OFFER
pratica negotiation cancel NEGOTIATION --reason TEXT
pratica steward respond NEGOTIATION
```

`propose` is an optimistic complete replacement: `--base` must be the current
offer at commit time. It never patches or merges prior Markdown. The submitting
party assents to the new offer and every other prior assent becomes stale.
Its optional request key binds the negotiation, expected base, and exact terms
identity. It is required when `--terms -`.

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
references without printing the persisted request, frozen source bodies, or
canonical source-origin paths.
`retry` is allowed only for a terminal completed, failed, cancelled, lost, or
timed-out attempt with no committed Pratica result. A completed attempt is
eligible only because Nucleus completion without the required accepted tool
submission is not Pratica success. Retry creates a successor attempt and job
ID; it never changes or reuses the prior Nucleus job. A blocked steward or
review response is a committed domain result, not a retryable runtime failure.

## Agreements

```sh
pratica agreement list
pratica agreement show AGREEMENT
pratica agreement export AGREEMENT [--output FILE]
pratica agreement verify AGREEMENT
pratica agreement amend AGREEMENT \
  --terms FILE_OR_DASH [--request-key KEY]
```

`list` is a body-free newest-first discovery view containing agreement,
integration, track, scope, negotiation, offer and basis identities; parties;
integration title; terms digest and byte count; basis freshness; amendment
links; and seal time. Exact terms remain available only through deliberate
detail or export commands.

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
unseals or overwrites the predecessor. Its optional request key binds the
agreement and exact successor terms identity and is required with `--terms -`.

## Conformance

```sh
pratica conformance review AGREEMENT \
  --candidate-basis MANIFEST_OR_DASH \
  [--source-root ABSOLUTE_DIRECTORY] [--request-key KEY]
pratica conformance show REVIEW
```

The review command freezes the supplied candidate basis, submits one bounded
Nucleus job, and commits exactly one immutable conformance review. `show` reads
that review and its agreement/candidate correlations. A conformance review is
evidence analysis only: it neither runs product tests nor implements, deploys,
releases, assents, or changes the sealed agreement.

A stdin candidate manifest has the same required `--source-root` behavior as a
stdin steward manifest and also requires `--request-key`. For conformance, the
receipt and candidate basis are admitted atomically before model execution.
An exact replay returns that basis and resumes or reports its existing attempt;
it never freezes a second candidate basis merely because the caller lost the
first command response.

## Failures and automation

Commands return nonzero for invalid input, missing/unsupported storage, stale
base or offer, conflicting identity reuse, unauthorized party action, retired
or closed state, basis drift, unavailable Nucleus, malformed tool calls,
terminal runtime failure, or missing domain success. Validation failures commit
no partial protocol event. A conflicting request-key reuse likewise commits
nothing.

Callers may recover integration and agreement identities through the body-free
list commands and should pass opaque identities back explicitly. They must not
infer agreement from Nucleus completion, agent prose, identical Markdown
digests, or a prior assent to a now-stale offer.

Machine output uses a common version-one envelope. Success is
`{"schema_version":1,"ok":true,"data":{"type":"..."}}`; failure is
`{"schema_version":1,"ok":false,"error":{"code":"...","message":"..."}}`.
Record identifiers are opaque even when current examples use readable prefixes.
