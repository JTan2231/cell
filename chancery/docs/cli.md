# CLI contract

```text
chancery [--registry PATH] [--json] list [--mode MODE] [--kind KIND]
chancery [--registry PATH] [--json] show ID
chancery [--registry PATH] [--json] doctor
chancery [--json] validate BUNDLE
```

The default registry is
`~/Library/Application Support/Chancery/providers`. `--registry` takes
precedence over `CHANCERY_REGISTRY`.

Every command is read-only. Chancery does not probe runtime readiness, execute
a documented interface, call a model, or access the network. Usage errors exit
2. Unreadable state, a missing requested entry, or an invalid doctor or
validation report exits 1. JSON output is one versioned envelope.

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

USE — ordinary outcome work

todo.concern.capture-and-route — Save and research a concern for later
  Save one actionable concern with its source, then research a pending proposal to attach it, create or revise a todo, unify duplicates, defer it, or dismiss it.
  capability · use · Todo 0.3.0 · supported · installed · compatible · not_checked

OPERATE — administration, diagnosis, and recovery

nucleus.execution.operate — Manage Nucleus agent jobs and service
  Check readiness and account access, submit or inspect agent jobs, read their output, cancel work, or operate the per-user Nucleus service.
  capability · operate · Nucleus 0.3.0 · supported · installed · compatible · not_checked
```

Each card contains the stable ID, title, summary, kind, mode, provider release,
owner-declared support, installed availability, contract compatibility, and
readiness classification. `availability=installed` means the indexed bundle is
present and valid. `compatibility=unavailable` means a declared contract
dependency is absent, out of range, cyclic, or transitively unavailable.
`readiness=not_checked` or `session_dependent` is deliberately not a live
health claim.

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

The human output begins:

```text
todo.concern.capture-and-route
Save and research a concern for later

Kind:              capability
Mode:              use
Owner:             Todo
Provider release:  0.3.0
Support:           supported
Availability:      installed
Compatibility:     compatible
Readiness:         not_checked
Contract version:  1

USE WHEN

  - The user wants an actionable concern or follow-up durably retained for later rather than completed now.

DO NOT USE WHEN

  - The user wants the work completed in the current request.
```

It continues with outcome, stable interfaces, effects, authority, success,
failure and recovery, privacy, non-authorizations, dependencies, session
surfaces, operation steps/checkpoints/adaptation/stop conditions when
applicable, and the complete installed Markdown manual. Reading `show` should
be enough to decide and carry out the represented request without inspecting
source code. It still does not prove live readiness or execute an interface.

## `doctor`

`doctor` validates the complete installed registry and cross-provider contract
dependencies:

```text
Chancery registry
  root: /Users/joey/Library/Application Support/Chancery/providers

PASS  annals        0.12.0  7 entries
PASS  annals-usage  0.4.0   3 entries
PASS  chancery      0.2.0   2 entries
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

The current provider schema is version 2. During coordinated migration the
reader also accepts schema-v1 bundles, discards their obsolete `routable` and
`routing` metadata, and presents the same catalog and `show` shape. New or
updated bundles must publish schema v2.

## JSON

`--json` writes exactly one compact JSON document and no ANSI or explanatory
prose. A catalog result has this shape:

```json
{"schema_version":2,"ok":true,"data":{"entries":[{"id":"todo.concern.capture-and-route","title":"Save and research a concern for later","summary":"Save one actionable concern with its source, then research a pending proposal to attach it, create or revise a todo, unify duplicates, defer it, or dismiss it.","kind":"capability","mode":"use","provider":{"id":"todo","name":"Todo","release":"0.3.0"},"provider_release":"0.3.0","contract_version":1,"support":"supported","availability":"installed","compatibility":"compatible","readiness":"not_checked"}],"issues":[]}}
```

Invalid doctor or validate reports retain the complete data report with
`"ok":false`. Command errors use stderr:

```json
{"schema_version":2,"ok":false,"error":{"code":"entry_not_found","message":"installed entry not found: missing.entry"}}
```

The JSON `show` result contains the complete current entry document, provider
identity, availability, compatibility, readiness, dependency statuses, manual
text, and registry issues. Neither list nor show emits legacy routing metadata.
Consumers should use the output schema version and named fields, not human
formatting. Output schema 2 removes request-resolution documents and adds the
complete catalog fields. The output-envelope schema is independent from the
provider schema.

## Exit status

| Result | Exit |
| --- | ---: |
| List or show success | 0 |
| Valid doctor or standalone bundle | 0 |
| Invalid doctor/bundle, unreadable registry, or missing entry | 1 |
| CLI usage | 2 |
