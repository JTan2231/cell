# CLI contract

```text
chancery [--registry PATH] [--json] list [--mode MODE] [--kind KIND]
chancery [--registry PATH] [--json] show ID [--full]
chancery [--registry PATH] [--json] resolve ID [--min-contract VERSION] [--max-contract-exclusive VERSION] [--require FACET]... [--summary]
chancery [--registry PATH] [--json] doctor
chancery [--json] validate BUNDLE
```

The default registry is
`~/Library/Application Support/Chancery/providers`. `--registry` takes
precedence over `CHANCERY_REGISTRY`.

Catalog and report computation preserve their source records. CLI dispatch separately attempts a command-usage append. Chancery does not test runtime readiness, execute
a documented interface, call a model, or access the network. Usage errors return
exit code 2. Unreadable state, a missing entry, or an invalid doctor or validation
report returns 1. An unresolved dossier also returns 1 and preserves its full
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

todo.concern.capture-and-route — Save and research a concern for later
  Save one actionable concern with its source, then research a pending proposal to attach it, create or revise a todo, unify duplicates, defer it, or dismiss it.

OPERATE — administration, diagnosis, and recovery

nucleus.execution.operate — Manage Nucleus agent jobs and service
  Check readiness and account access, submit or inspect agent jobs, read their output, cancel work, or operate the per-user Nucleus service.
```

Each card contains the stable ID, title, and summary. JSON, groups, and filters
also expose kind and mode. Shared support, availability, compatibility, and
readiness appear once under `defaults`. Cards show exceptions. Use `show` for
the provider release and contract version.
`availability=installed` means valid indexed documentation, and
`compatibility=unavailable` means a missing, incompatible or cyclic dependency.
Readiness is never probed. Operation readiness remains `session_dependent`.

To narrow the result, use `--mode use|operate|develop` or
`--kind capability|operation`. Plain `list` returns the complete registered
inventory. There is no separate `--all` mode.

The interactive agent uses the titles and summaries to form a semantic
shortlist. Chancery does not receive the user's request and does not choose an
entry.

## `show`

After identifying one or more plausible entries, read each complete contract:

```sh
/Users/joey/.local/bin/chancery show todo.concern.capture-and-route
```

`show` prints identity, release, support, availability, compatibility, readiness,
dependency statuses, and the complete operating manual once. JSON contains the
same identity and manual. The manual must state applicability, exact interfaces,
effects, authority, success, recovery, privacy, exclusions, and required
operation checkpoints. `show` neither tests readiness nor executes an interface.

`show ID --full` includes the original structured authoring fields and
normalized claims as well as the manual. Use it to inspect authoring or compare
declarations. `resolve` remains the full outward-promise and dependency read.

## `resolve`

After discovery and `show` have selected one exact entry, resolve its complete
outward-promise dossier:

```sh
/Users/joey/.local/bin/chancery resolve decisions.lifecycle.consume
```

`resolve` accepts one stable installed ID. The interactive agent selects that
ID. User requests, keywords, provider guesses, and ranking criteria are not
accepted. Extra positional text causes a usage error.

Outcome and gaps precede the contract bodies in human output.
`resolve ID --summary` returns only outcome, requirements, declaration and
closure status, readiness, gaps and issues. It accepts the same bounds and
facets and returns the same exit status as the full resolution.

The full dossier contains:

- the root provider identity, release, schema, promise scope, complete entry
  and manual, normalized facet coverage, direct dependency status,
  availability, compatibility, and readiness;
- the same complete dossier for every installed transitive documentation
  dependency, once, in stable ID order;
- provider-manifest, entry, and manual bundle paths plus SHA-256 digests of the
  exact raw UTF-8 bytes read;
- optional contract-bound and required-positive-facet results; and
- registry issues and explicit gaps.

Full resolution requires provider scope and a complete normalized entry
declaration. For schema-1, schema-2, and partially onboarded entries, resolution
still returns the existing contracts. Missing scope and normalized facets are
`undeclared`. Claims retain `declared`, `unsupported`, `unspecified`, or
`not_applicable` status. A facet with several statuses is `mixed`. The resolver
does not infer support from manual prose, database schemas, tests, or code.

Use optional bounds when a consumer needs a particular root contract family:

```sh
chancery resolve decisions.lifecycle.consume \
  --min-contract 1 \
  --max-contract-exclusive 2
```

Use repeatable `--require` values when a design needs positive root claims:

```sh
chancery resolve decisions.lifecycle.consume \
  --require data_semantics \
  --require completeness_and_freshness
```

The supported facet names are `applicability`, `outcome`, `consumers`,
`preconditions`, `interfaces`, `inputs`, `outputs`, `data_semantics`,
`identity_and_units`, `completeness_and_freshness`, `effects`, `authority`,
`access`, `lifecycle_and_consistency`, `success`, `failure_and_recovery`,
`privacy`, `operational_limits`, `compatibility_and_evolution`,
`dependencies`, `reliances`, and `exclusions`. A requirement is satisfied only
when that root facet contains a positive `declared` claim.

Resolution status is:

| Status | Meaning |
| --- | --- |
| `resolved_not_ready` | The documentary promise and dependency closure resolve; live readiness remains separately unchecked. |
| `incomplete_declaration` | Scope, normalized claims, a dedicated substantive-reliance contract, or a required positive facet is absent. |
| `dependency_unavailable` | A documentation dependency is missing, out of range, cyclic, or transitively unavailable. |
| `contract_incompatible` | The root entry falls outside caller-supplied contract bounds. |

`unsupported` and `not_applicable` claims define boundaries. `unspecified`
claims appear as gaps but do not make the document structurally incomplete.
A declared substantive reliance without a dedicated versioned contract is an
`uncontracted_reliance`. Ordinary `dependencies` describe contract compatibility;
the resolver does not infer runtime calls or data flow from them.

`resolved_not_ready` returns exit code 0. Other resolution statuses return the
full dossier on stdout and exit code 1. Resolution does not test readiness,
execute an interface, or grant authorization.

## `doctor`

`doctor` validates the complete installed registry and cross-provider contract
dependencies:

```text
Chancery registry
  root: /Users/joey/Library/Application Support/Chancery/providers

PASS  annals        0.12.0  7 entries
PASS  annals-usage  0.4.0   3 entries
PASS  chancery      0.2.0   3 entries
PASS  nucleus       0.3.0   3 entries
PASS  todo          0.3.0   7 entries

Providers: 5 valid, 0 excluded
Entries:   23
Status:    valid
```

An invalid provider is excluded and reported under `ISSUES`; valid providers
remain queryable. Missing, out-of-range, transitively unavailable, or cyclic
dependencies make doctor invalid. `doctor` never runs a product health command,
checks an account, or contacts a service.

## `validate`

Product CI validates a standalone source bundle without an installed registry:

```text
$ chancery validate /absolute/path/to/todo/chancery
PASS  Todo 0.3.0
Bundle: /absolute/path/to/todo/chancery
Entries: 7
External dependencies: not checked
```

Structural failures print every detected issue and exit 1. Dependencies that
name another entry in the same bundle are checked for contract-version
compatibility and cycles. Cross-provider dependencies are deliberately
reported as `not_checked`; installed compatibility belongs to `doctor`.

The current provider schema is version 3. During coordinated migration the
reader also accepts schema-1 and schema-2 bundles. It discards obsolete
`routable` and `routing` metadata only for schema 1 and presents the same
catalog and `show` shape. New or updated bundles publish schema 3 with a
provider promise scope; older entries resolve with explicit normalized gaps.

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

Output schema 3 defines compact list and ordinary show results.
`FullShowResult` contains the complete entry for `--full`. `ResolveResult`
contains the full dossier; `ResolveSummary` contains its outcome and gaps.
Use the provider-owned Rust client and named fields. `--json` changes encoding
only. Provider schemas are separate.

## Exit status

| Result | Exit |
| --- | ---: |
| List, show, or fully documented resolve success | 0 |
| Valid doctor or standalone bundle | 0 |
| Incomplete/incompatible resolve, invalid doctor/bundle, unreadable registry, or missing entry | 1 |
| CLI usage | 2 |
