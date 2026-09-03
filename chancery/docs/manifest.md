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

New and updated providers use schema version 3. `provider.json` is UTF-8 JSON
with no unknown fields:

```json
{
  "schema_version": 3,
  "provider": {
    "id": "example",
    "name": "Example",
    "release": "2.4.1"
  },
  "promise_scope": {
    "authoritative_for": ["Example owns validated report state."],
    "not_authoritative_for": ["The caller owns publication decisions."],
    "inventory": {
      "covers": ["All supported public Example CLI outcomes in this release."],
      "completeness": "complete",
      "excludes": ["Help, internals, and live readiness."]
    },
    "shared_access_and_trust": ["Interfaces are local-user surfaces."],
    "shared_privacy_and_retention": ["Per-entry retention rules apply."],
    "compatibility_and_retirement": ["Entry versions identify compatibility."],
    "operational_limits": ["Per-entry quantitative bounds apply."]
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

`promise_scope` is the provider's outward-facing jurisdiction and inventory
boundary. Every collection is nonempty. `inventory.covers` names a meaningful
class of supported surfaces; it must not define the class circularly as
“whatever is indexed.” `completeness` is `complete` or `partial` within that
named class. `inventory.excludes` makes clear what absence cannot decide.

The remaining fields state what the provider is and is not authoritative for
and the common access/trust, privacy/retention, compatibility/retirement, and
operational-limit qualifiers shared by its entries. Entry-specific facts still
belong in the entry and manual. Provider scope is not runtime proof, global
ecosystem completeness, or an authorization grant.

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
  "promise": {
    "consumers": [
      {"status": "declared", "statement": "Local report readers may use it."}
    ],
    "preconditions": [
      {"status": "declared", "statement": "Validated inputs are available."}
    ],
    "inputs": [
      {"status": "declared", "statement": "One validated report request."}
    ],
    "outputs": [
      {"status": "declared", "statement": "One current report identity."}
    ],
    "data_semantics": [
      {"status": "declared", "statement": "Current means owner-selected and valid."}
    ],
    "identity_and_units": [
      {"status": "declared", "statement": "Report ID identifies the retained report."}
    ],
    "completeness_and_freshness": [
      {"status": "declared", "statement": "Success selects the complete validated report."},
      {"status": "unspecified", "statement": "No build-latency SLA is promised."}
    ],
    "access": [
      {"status": "declared", "statement": "The supported surface is the local CLI."}
    ],
    "lifecycle_and_consistency": [
      {"status": "declared", "statement": "Failure preserves the prior current report."}
    ],
    "operational_limits": [
      {"status": "unspecified", "statement": "No report-size limit is promised here."}
    ],
    "compatibility_and_evolution": [
      {"status": "declared", "statement": "Contract version identifies semantics."}
    ],
    "reliances": [
      {
        "status": "declared",
        "statement": "Report building uses the other execution capability.",
        "target": "other",
        "kind": "control",
        "contract": "other.execution.operate"
      }
    ]
  },
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
| `promise` | Optional schema-3 normalized outward-boundary declaration; complete when present |
| `manual` | Indexed, nonempty, UTF-8 detailed Markdown beneath the bundle |

Titles and summaries are part of the functional discovery contract. They must
distinguish the entry by intended result, not merely repeat an internal command
name or a generic noun. Chancery lists them verbatim; semantic comparison with
the user's request belongs to the interactive agent.

Collections required by the selected entry kind must not be empty. Values in
any collection must be nonblank and unique. IDs, indexed paths, and dependency
ranges must also be unique and valid. Unknown fields are rejected so that
misspellings cannot silently weaken a contract.

## Normalized promise declaration

The optional schema-3 `promise` object is all-or-nothing. When present, each of
these collections contains at least one explicit claim:

| Facet | Question answered |
| --- | --- |
| `consumers` | Who can rely on this promise? |
| `preconditions` | What must already be true? |
| `inputs` / `outputs` | What crosses the supported surface? |
| `data_semantics` | What do values and states mean? |
| `identity_and_units` | What identifies records, and what is measured? |
| `completeness_and_freshness` | What coverage and currentness are promised? |
| `access` | Through what trust and access boundary is it available? |
| `lifecycle_and_consistency` | What ordering, atomicity, replay, and recovery model applies? |
| `operational_limits` | What material bounds or absent bounds qualify it? |
| `compatibility_and_evolution` | How are versions, migration, deprecation, and retirement handled? |
| `reliances` | What substantive external data, control, authority, readiness, or external source does the outcome rely on? |

A normal claim has `status` and `statement`. Status is `declared`,
`unsupported`, `unspecified`, or `not_applicable`. Multiple claims may give a
facet mixed status. `unsupported` is an explicit negative boundary;
`unspecified` says the owner makes no guarantee; `not_applicable` says the
question does not fit the capability. If the entire `promise` object is absent,
resolution reports these facets as `undeclared` instead of searching the
manual, schema, tests, or implementation for an inferred answer.

A `reliances` claim with status `declared` additionally requires a lowercase
`target` provider/system ID and a `kind` of `data`, `control`, `authority`,
`readiness`, or `external`. It may name a `contract`; when it does, that entry
must also appear in `dependencies` with explicit version bounds. A declared
reliance without a contract is valid but resolves as an
`uncontracted_reliance` gap. Non-declared reliance claims do not carry target,
kind, or contract metadata.

This distinction is deliberate. `dependencies` always mean installed
documentation-contract compatibility. Chancery never mechanically treats an
existing dependency as proof of a runtime call, private data surface, authority
transfer, or readiness relationship.

The exact entry and manual remain the detailed promise. Resolution cites the
raw UTF-8 `provider.json`, entry, and manual bytes by bundle-relative path and
lowercase SHA-256 digest; normalized fields do not create a second truth store.

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

## Legacy schemas 1 and 2

The reader temporarily accepts provider schema v1 so independently deployed
products can migrate after Chancery. Its entry documents may contain the old
`routable` and `routing` fields. Chancery ignores those two fields completely:
they do not filter the catalog, influence selection, or appear in list/show
output. All other v1 fields retain the same validation rules.

Schema v2 removes both fields. A v2 entry containing either one is invalid.
Schemas 1 and 2 cannot contain `promise_scope` or `promise`; exact-ID
resolution preserves their full existing documents and reports provider scope
and normalized facets as undeclared.

Providers migrate by first deploying a reader that accepts schema 3, then
changing `schema_version` to 3, adding a complete `promise_scope`, and
normalizing entries deliberately. Entry `promise` remains optional so one
provider can onboard in bounded steps. Unknown schema-3 fields and incomplete
scope or promise objects are invalid.

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
