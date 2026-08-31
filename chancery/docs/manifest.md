# Provider manifest

One provider bundle has this shape:

```text
provider.json
entries/
  ENTRY.json
manuals/
  ENTRY.md
```

`provider.json` explicitly indexes every entry. Chancery never discovers
unlisted drafts, source-tree documentation, or arbitrary files. Registry-level
provider symbolic links are allowed. Symbolic links in an indexed path are
rejected, and every indexed path must remain beneath that fixed bundle root.
Product packaging applies a stricter whole-tree check before publication.

Schema version, provider release, and each entry contract version are
independent. Dependencies name stable entry IDs and integer contract-version
bounds. Operations additionally declare session surfaces whose live
availability must be checked by the interactive agent.

## Provider file

New and updated providers use schema version 2. `provider.json` is UTF-8 JSON
with no unknown fields:

```json
{
  "schema_version": 2,
  "provider": {
    "id": "example",
    "name": "Example",
    "release": "2.4.1"
  },
  "entries": [
    "entries/report-build.json"
  ]
}
```

The provider ID is stable, lowercase ASCII, and must match the selector name
in the installed registry. `release` identifies the product release containing
these exact bytes. Every indexed path is unique, relative, and remains inside
the bundle. The reader ignores every unindexed file.

## Capability entry

```json
{
  "id": "example.report.build",
  "contract_version": 1,
  "kind": "capability",
  "mode": "use",
  "support": "supported",
  "title": "Build a report",
  "summary": "Build and retain one current report from validated inputs.",
  "use_when": ["The current report should be regenerated."],
  "do_not_use_when": ["The user only wants an explanation."],
  "outcome": "The product retains a validated current report.",
  "effects": ["Writes product-owned report state."],
  "authority": ["The product record, not process exit alone, proves success."],
  "success": ["The product reports the new current report as valid."],
  "failure_and_recovery": ["The prior current report remains selected on failure."],
  "privacy": ["Validated input may be retained in product state."],
  "does_not_authorize": ["Publishing or distributing the report."],
  "interfaces": [
    {"label": "Build", "invocation": "/absolute/path/example build"}
  ],
  "dependencies": [
    {"id": "other.execution.operate", "min_contract": 1, "max_contract_exclusive": 2}
  ],
  "manual": "manuals/report-build.md"
}
```

Common field meanings:

| Field | Meaning |
| --- | --- |
| `id` | Globally unique stable entry ID, prefixed by its provider ID |
| `contract_version` | Positive integer version of documented semantics, separate from product release |
| `kind` | `capability` or `operation` |
| `mode` | `use`, `operate`, or `develop` audience boundary |
| `support` | `supported` or `deprecated`, declared by the owner |
| `title` | Short, discriminative name shown in the complete catalog |
| `summary` | Concise user-visible result used by an agent to form a semantic shortlist |
| `use_when` / `do_not_use_when` | Detailed semantic boundary inspected before selection |
| `outcome` | Durable or observable result the user is asking for |
| `effects` | Writes, calls, disclosure, usage, or other consequences of the real interface |
| `authority` | Which record or system decides behavior and success |
| `success` | Evidence required after actual invocation |
| `failure_and_recovery` | Partial success, preserved state, retry, and repair boundary |
| `privacy` | What the actual capability may retain or disclose; an explicit no-retention statement is valid |
| `does_not_authorize` | Optional explicit authority boundary for a capability; required and nonempty for an operation |
| `interfaces` | Display-only stable invocations; Chancery never executes them |
| `dependencies` | Required installed documentation contracts and integer version range |
| `manual` | Indexed, nonempty, UTF-8 detailed Markdown beneath the bundle |

Titles and summaries are part of the functional discovery contract. They must
distinguish the entry by intended result, not merely repeat an internal command
name or a generic noun. Chancery lists them verbatim; semantic comparison with
the user's request belongs to the interactive agent.

Collections required by the selected entry kind must not be empty. Values in
any collection must be nonblank and unique. IDs, indexed paths, and dependency
ranges must also be unique and valid. Unknown fields are rejected so that
misspellings cannot silently weaken a contract.

## Operation additions

An operation uses the common field set, requires a nonempty
`does_not_authorize` list, and additionally declares:

```json
{
  "session_surfaces": ["browser", "computer_use"],
  "runtime": "interactive_agent",
  "automation": "none",
  "steps": ["Observe the current semantic UI state."],
  "checkpoints": ["Confirm the selected target before drafting."],
  "adaptation": ["Relocate controls by visible meaning rather than selectors."],
  "stop_when": ["Login, MFA, CAPTCHA, legal attestation, or unsupported input requires the user."]
}
```

These operation-only fields describe an adaptive operational manual. They do
not create a workflow engine, grant computer access, or imply that named
session surfaces are installed or ready. The detailed manual should organize
the same operation around goals, participants, semantic actions, proof,
recovery, and authority checkpoints instead of volatile selectors or pixel
coordinates.

Capability entries must not populate operation-only fields and must declare at
least one stable interface. Operations require every operation-only field and a
nonempty `does_not_authorize` list, but may omit interfaces when there is no
stable direct invocation. Both kinds may have zero dependencies.

## Legacy schema v1

The reader temporarily accepts provider schema v1 so independently deployed
products can migrate after Chancery. Its entry documents may contain the old
`routable` and `routing` fields. Chancery ignores those two fields completely:
they do not filter the catalog, influence selection, or appear in list/show
output. All other v1 fields retain the same validation rules.

Schema v2 removes both fields. A v2 entry containing either one is invalid.
Providers should migrate by changing `schema_version` to 2 and deleting the
obsolete keys; titles, summaries, semantic boundaries, and manuals must carry
the discovery meaning.

## Validation and security

Registry-level provider selectors may be symbolic links so they can follow a
product's current content-addressed release. Chancery canonicalizes each once
per invocation. CLI validation reads only `provider.json` and the entry and
manual paths it indexes. Every indexed path component must remain beneath the
resolved root and may not be a symbolic link; indexed devices, sockets, other
non-files, invalid UTF-8, oversized inputs, unsupported schemas, duplicate
entry IDs, dependency cycles, and impossible version ranges are rejected.
Unindexed objects do not affect CLI validation. Product packaging and
deployment checks additionally require the entire published bundle tree to
contain only regular files and directories before hashing and staging it.

Validation treats every invocation string as inert text. A bundle cannot cause
Chancery to run a command, probe a service, open a network connection, or call
a model.
