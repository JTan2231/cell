# Installed catalog and contract reading

Chancery reads product-owned versioned documentation. Each invocation fixes
one canonical bundle for each provider selector and builds its view in memory.
It retains no catalog database or daemon state. A malformed provider is
excluded as a unit; valid providers remain readable. Duplicate global entry IDs
are excluded independently of filesystem order. A missing or incompatible
contract dependency remains visible and propagates to dependent entries.

The owning product decides what its documented interface does and what proves
domain success. Chancery owns structural validation, installed enumeration,
compatibility reporting, and display. The interactive caller compares user
intent with the catalog, reads every plausible page, and invokes a selected
interface separately. Catalog presence does not authorize work or establish
live readiness. Chancery does not match user requests, call a model, access the
network, or execute represented interfaces.

For routine selection, read the complete `list`, compare all titles and
summaries with the intended outcome, and read every plausible entry with
`show`. If no entry fits, proceed normally. Use `product` when the owner is
already known. Use `resolve` after selecting one exact ID when the request
concerns a full outward promise or design reliance; read
`chancery show chancery.capability.resolve` for its complete behavior. Required
feature dependencies enter its closure. Related Markdown references are
navigation only. Read `chancery show chancery.bundle.validate` for bundle shape,
path rules, validation bounds, and schema evolution.

## Commands and registry

```text
chancery [--registry PATH] [--json] list [--provider PROVIDER_ID] [--mode MODE] [--kind KIND]
chancery [--registry PATH] [--json] product PROVIDER_ID
chancery [--registry PATH] [--json] show ID [--full]
chancery [--registry PATH] [--json] doctor
```

The default registry is
`~/Library/Application Support/Chancery/providers`. `--registry` takes
precedence over `CHANCERY_REGISTRY`.

Catalog and report computation preserve their source records. CLI dispatch separately attempts a command-usage append. Chancery does not test runtime readiness, execute
a documented interface, call a model, or access the network. CLI syntax errors return
exit code 2. Unreadable state, a missing provider or entry, or an invalid doctor
or validation report returns 1. An unresolved dossier also returns 1 and preserves its full
result for inspection. JSON output uses one versioned envelope.

The output below shows example formats. The selected registry supplies provider
releases, installed entries, and counts.

## `list`

Use `list` for discovery. Without filters, it reports every entry from each
structurally valid installed provider. This includes deprecated entries and
entries with unavailable contract dependencies. Registry problems appear under
`ISSUES` and in the JSON `issues` collection, even when other providers remain
usable.

Human output groups entries by ordinary work, administration, and development:

```text
Installed Chancery catalog
Defaults: supported · installed · compatible · readiness not_checked (not probed). Exceptions appear below.

USE — ordinary outcome work

example.read — Read saved examples
  Inspect retained examples without changing them.

OPERATE — administration, diagnosis, and recovery

example.recover — Recover saved examples
  Verify and restore compatible saved example history.
```

Each card contains the stable ID, title, and summary. JSON, groups, and filters
also expose kind and mode. Shared support, availability, compatibility, and
readiness appear once under `defaults`. Cards show exceptions. Use `show` for
the provider release and contract version.
`availability=installed` means valid indexed documentation, and
`compatibility=unavailable` means a missing, incompatible or cyclic dependency.
Readiness is never probed. Operation readiness remains `session_dependent`.

The cards above illustrate the format; they do not assert an installed provider.

To narrow the result, use `--provider PROVIDER_ID`,
`--mode use|operate|develop`, or `--kind capability|operation`. Filters combine.
The provider filter uses an exact provider ID and preserves registry issues.
An unknown or excluded provider returns `provider_not_found`. Plain `list`
returns the complete registered inventory. There is no separate `--all` mode.

The interactive agent uses the titles and summaries to form a semantic
shortlist. Chancery does not receive the user's request and does not choose an
entry.

## `product`

Read one installed product's overview and inventory:

```sh
chancery product nucleus
chancery list --provider nucleus
```

`product` accepts one exact provider ID. It shows provider identity, release,
schema version, promise scope when published, the authored overview when
present, and every installed entry
owned by that provider. Inventory cards use the same shared defaults and
exceptions as `list`. Registry issues remain visible. An unknown or excluded
provider returns `provider_not_found`.

The overview comes only from the manifest's indexed Markdown file. It is
optional in schema 4 and unavailable in earlier schemas. An absent overview
returns `overview_status: not_published` and a null `overview` in JSON; the
provider inventory remains available. A present overview returns
`overview_status: published` and its complete text. This status describes
published documentation, not live product readiness.

The overview provides context and navigation. Use `show` for a feature's
complete page or a procedure's operating essentials. Use `resolve` for the
selected entry and its required feature contracts. Product navigation neither
executes an interface nor adds a separate entry contract or dependency.

## `show`

After identifying one or more plausible entries, read each complete contract:

```sh
/Users/joey/.local/bin/chancery show example.read
```

`show` prints identity, release, support, availability, compatibility, readiness,
dependency statuses, and the complete operating manual once. JSON contains the
same identity and manual. The manual must state applicability, exact interfaces,
effects, authority, success, recovery, privacy, exclusions, and required
operation checkpoints. A feature page owns the detailed explanation of its
capability. A procedure can require feature contracts for that explanation,
while retaining all conditions needed to carry out its own steps. `show`
neither tests readiness nor executes an interface.

`show ID --full` includes the original structured authoring fields and
normalized claims as well as the manual. Use it to inspect authoring or compare
declarations. `resolve` remains the full outward-promise and dependency read.

## `doctor`

`doctor` validates the complete installed registry and cross-provider contract
dependencies:

```text
Chancery registry
  root: /absolute/path/to/providers

PASS  example  2.4.1  2 entries

Providers: 1 valid, 0 excluded
Entries:   2
Status:    valid
```

An invalid provider is excluded and reported under `ISSUES`; valid providers
remain queryable. Missing, out-of-range, transitively unavailable, or cyclic
dependencies make doctor invalid. `doctor` never runs a product health command,
checks an account, or contacts a service.

## JSON

`--json` writes exactly one compact JSON document and no ANSI or explanatory
prose. A catalog result has this shape:

```json
{"schema_version":3,"ok":true,"data":{"defaults":{"support":"supported","availability":"installed","compatibility":"compatible","readiness":"not_checked"},"entries":[{"id":"example.read","title":"Read examples","summary":"Inspect saved examples.","kind":"capability","mode":"use"}],"issues":[]}}
```

Invalid doctor or validate reports retain the complete data report with
`"ok":false`. Command errors use stderr:

```json
{"schema_version":3,"ok":false,"error":{"code":"entry_not_found","message":"installed entry not found: missing.entry"}}
```

Output schema 3 defines compact list and ordinary show results. The additive
`ProductResult` contains `provider`, `provider_schema_version`,
`promise_scope` (null for legacy providers), `overview`, `overview_status`,
status `defaults`, `entries`, and `issues`.
`FullShowResult` contains the complete entry for `--full`. `ResolveResult`
contains the full dossier; `ResolveSummary` contains its outcome and gaps.
Use the provider-owned Rust client and named fields. `--json` changes encoding
only. Provider schemas are separate.

## Exit status

| Result | Exit |
| --- | ---: |
| List, product, show, or fully documented resolve success | 0 |
| Valid doctor or standalone bundle | 0 |
| Incomplete/incompatible resolve, invalid doctor/bundle, unreadable registry, or missing provider/entry | 1 |
| CLI usage | 2 |

## Rust interface

Rust callers use `chancery::api::Client::new(executable)` and optional
`with_registry(registry)`. The supported methods are `list(mode, kind)`,
`list_provider(provider, mode, kind)`, `product(provider)`, `show(id)`,
`show_full(id)`, `resolve(id, min_contract, max_contract_exclusive, require)`,
`resolve_summary(id)`, `doctor()`, and `validate(bundle)`. The caller selects
the executable and registry; the client adds `--json` and marks this dependency
process internal for usage recording.

Each method returns `Result<Output<T>, ClientError>`. An invalid doctor or
validate report, or unresolved promise, is an inspectable report with
`ok: false`. `ClientError::Io`, `Json`, `Provider { code, message }`, and
`Protocol` distinguish process access, decoding, provider command errors, and
unsupported or inconsistent envelopes. The client accepts output schema 3 and
requires envelope `ok` to agree with the process status.

`Output<T>` has `schema_version`, `ok`, and `data`. `ListResult` has `defaults`,
`entries`, and `issues`. `CatalogEntry` has `id`, `title`, `summary`, `kind`, and
`mode`; optional `support`, `compatibility`, and `readiness` carry exceptions
only. `CatalogDefaults` has `support`, `availability`, `compatibility`, and
`readiness`. `ShowResult` has `provider`, `entry`, `availability`,
`compatibility`, `readiness`, `dependency_statuses`, `manual`, and `issues`.
Its `entry` is `EntryIdentity`: `id`, `title`, `kind`, `mode`,
`contract_version`, and `support`. `FullShowResult` substitutes the complete
`EntryDocument`. `DoctorResult` has `valid`, `registry`, `providers`, `counts`,
and `issues`. Its counts are `scanned_providers`, `valid_providers`,
`excluded_providers`, and `entries`. Each provider summary has `id`, `name`,
`release`, `root`, and `entries`.

The `api` module also exports provider-owned document types and codecs.
`ProviderManifest::decode(text)` and `EntryDocument::decode(text, schema_version)`
use the CLI codecs, including schema-one legacy handling. Decoding checks the
selected shape; it does not establish full bundle validity, installed presence,
readiness, or authority. `ProviderIntroduction` and `EntryIntroduction` are
partial identity and indexed-manual views for Usher. They ignore unrelated
fields and do not evaluate promises or dependencies. Usher owns membership
policy. Do not copy private parser or wire definitions into a consumer.

## Privacy, limits, and recovery

Documentation is processed locally and is not retained as a catalog snapshot.
Provider bundles must contain no credentials, secrets, private source material,
or transient output. Registry and filesystem permissions form the local
access boundary. Each invocation rereads the selected installed registry;
there is no timestamped cache or runtime observation. No wall-clock discovery
latency or registry-size service objective is promised beyond documented
validation bounds. Legacy schemas have no promised retirement date.

Use `doctor` to locate an excluded provider or unavailable dependency, validate
the owning source bundle, and repair or redeploy that product. Read its live
operating interface when readiness matters. An unknown entry returns
`entry_not_found`; an unknown or excluded provider returns `provider_not_found`.
Do not repair content-addressed installed documents in place. Read
`chancery show chancery.provider.publish` for publication and
`chancery show chancery.installation.operate` for reader recovery.

## Command usage

CLI dispatch separately attempts a command-usage append. A nonempty
`CODEX_THREAD_ID` is required. Internal calls are excluded, and recording errors
preserve the query result. No arguments or output enter the journal. Read
`chancery show chancery.usage.record` for the complete usage feature and
`chancery show chancery.usage.operate` for registration and recovery.
