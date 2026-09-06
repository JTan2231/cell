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

Every command is read-only. Chancery does not probe runtime readiness, execute
a documented interface, call a model, or access the network. Usage errors exit
2. Unreadable state, a missing requested entry, or an invalid doctor or
validation report exits 1. An unresolved dossier also exits 1 while retaining
its complete inspectable result. JSON output is one versioned envelope.

The output excerpts below are schematic. Provider releases, installed entries,
and aggregate counts come from the selected registry and are not a maintained
inventory in this document.

## `list`

`list` is the discovery surface. With no filters it reports every entry from
every structurally valid installed provider, including deprecated entries and
entries whose contract dependencies are unavailable. Registry problems are
shown under `ISSUES` and in the JSON `issues` collection; they are never hidden
because some providers remain usable.

Human output is grouped by audience so ordinary work, administration, and
development are visibly distinct:

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

Each card contains the stable ID, title and summary. Kind and mode remain
available in JSON and grouping/filtering. Common support, availability,
compatibility and readiness appear once under `defaults`; cards carry only
exceptions. Provider release and contract version belong to `show`.
`availability=installed` means valid indexed documentation, and
`compatibility=unavailable` means a missing, incompatible or cyclic dependency.
Readiness is never probed. Operation readiness remains `session_dependent`.

Use `--mode use|operate|develop` or `--kind capability|operation` only when a
caller deliberately wants a narrower view. Plain `list` is always the complete
registered inventory; there is no separate `--all` mode.

The interactive agent uses the titles and summaries to form a semantic
shortlist. Chancery does not receive the user's request and does not choose an
entry.

## `show`

After identifying one or more plausible entries, read each complete contract:

```sh
/Users/joey/.local/bin/chancery show todo.concern.capture-and-route
```

`show` prints identity, release, support, availability, compatibility,
readiness, dependency statuses, and the complete authored operating manual
once. JSON carries the same selected identity and manual. The manual must
contain applicability, exact interfaces, effects, authority, success, recovery,
privacy, exclusions, and any operation checkpoints needed to act correctly.
It does not prove readiness or execute an interface.

`show ID --full` includes the original structured authoring fields and
normalized claims as well as the manual. Use it to inspect authoring or compare
declarations. `resolve` remains the full outward-promise and dependency read.

## `resolve`

After discovery and `show` have selected one exact entry, resolve its complete
outward-promise dossier:

```sh
/Users/joey/.local/bin/chancery resolve decisions.lifecycle.consume
```

`resolve` accepts exactly one stable installed ID. It does not accept a user
request, keywords, provider guess, or ranking criteria. Candidate selection
remains with the interactive agent; extra positional text is a usage error.

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

Provider scope and a complete normalized entry declaration are required for a
fully resolved dossier. Schema-1, schema-2, and partially onboarded entries
still resolve their existing full contracts, but absent scope and normalized
facets are `undeclared`. Claim status remains `declared`, `unsupported`,
`unspecified`, or `not_applicable`; a facet with several statuses is `mixed`.
Silence is never converted into support from manual prose, a database schema,
tests, or implementation code.

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

Explicit `unsupported` and `not_applicable` claims are boundaries rather than
gaps. Explicit `unspecified` claims are listed as gaps without making the
authored document structurally incomplete. A substantive declared reliance
without a dedicated versioned contract is an `uncontracted_reliance`; ordinary
`dependencies` never acquire runtime or data-flow meaning by inference.

`resolved_not_ready` exits 0. The other resolution statuses return the full
dossier on stdout and exit 1 so automation cannot silently treat them as a
complete promise. Resolution never probes readiness, executes an interface,
or grants authorization.

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
PASS  weaver        0.1.0   3 entries

Providers: 6 valid, 0 excluded
Entries:   25
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

Output schema 3 versions the compact list and ordinary show projections.
`FullShowResult` retains the complete entry through `--full`; `ResolveResult`
retains the full dossier, and `ResolveSummary` carries outcome and gaps only.
Use the provider-owned Rust client and named fields. `--json` changes encoding,
not the selected content. Provider schemas remain independent and unchanged.

## Exit status

| Result | Exit |
| --- | ---: |
| List, show, or fully documented resolve success | 0 |
| Valid doctor or standalone bundle | 0 |
| Incomplete/incompatible resolve, invalid doctor/bundle, unreadable registry, or missing entry | 1 |
| CLI usage | 2 |
