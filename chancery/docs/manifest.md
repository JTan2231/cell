# Provider manifest

One provider bundle has this shape:

```text
provider.json
entries/
  ENTRY.json
manuals/
  ENTRY.md
```

`provider.json` indexes every entry. Chancery reads indexed files only.
Provider selectors in the registry can be symbolic links. Indexed paths cannot
contain symbolic links and must stay beneath the fixed bundle root. Product
packaging checks the entire bundle tree before publication.

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

`promise_scope` defines the provider's jurisdiction and inventory scope. Every
collection must be nonempty. `inventory.covers` names a class of supported
surfaces independently of the index. `completeness` is `complete` or `partial`
within that class. `inventory.excludes` states which outcomes fall outside it.

The other fields define authority, access, trust, privacy, retention,
compatibility, retirement, and operational limits shared by the provider's
entries. Put entry-specific facts in the entry and manual. Provider scope
describes the published inventory. Runtime checks and authorization use the
documented interface.

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

Titles and summaries support discovery. They must distinguish entries by their
intended results. An internal command name or generic noun is insufficient.
Chancery displays them verbatim. The interactive agent compares their meaning
with the user's request.

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
| `preconditions` | What inputs and operating conditions are required? |
| `inputs` / `outputs` | What crosses the supported surface? |
| `data_semantics` | What do values and states mean? |
| `identity_and_units` | What identifies records, and what is measured? |
| `completeness_and_freshness` | Which selected records or operation does the result cover, and what do its timestamps mean? |
| `access` | Through what trust and access boundary is it available? |
| `lifecycle_and_consistency` | What ordering, atomicity, replay, and recovery model applies? |
| `operational_limits` | What material bounds or absent bounds qualify it? |
| `compatibility_and_evolution` | How are versions, migration, deprecation, and retirement handled? |
| `reliances` | What substantive external data, control, authority, readiness, or external source does the outcome rely on? |

A normal claim has `status` and `statement`. Status is `declared`,
`unsupported`, `unspecified`, or `not_applicable`. A facet can have mixed
statuses. `unsupported` defines an exclusion. `unspecified` leaves the behavior
unpromised. `not_applicable` means the question does not apply. If `promise` is
absent, the resolver reports these facets as `undeclared`. It does not infer
answers from the manual, schema, tests, or code.

A `reliances` claim with status `declared` additionally requires a lowercase
`target` provider/system ID and a `kind` of `data`, `control`, `authority`,
`readiness`, or `external`. It may name a `contract`; when it does, that entry
must also appear in `dependencies` with explicit version bounds. A declared
reliance without a contract is valid but resolves as an
`uncontracted_reliance` gap. Non-declared reliance claims do not carry target,
kind, or contract metadata.

`dependencies` describe installed documentation-contract compatibility. They
do not establish runtime calls, private data access, authority transfer, or
readiness relationships.

The exact entry and manual remain the detailed promise. Resolution cites the
raw UTF-8 `provider.json`, entry, and manual bytes by bundle-relative path and
lowercase SHA-256 digest. Normalized fields describe those same product-owned
contracts.

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

These fields describe an adaptive operation. They do not execute a workflow,
grant computer access, or establish that session surfaces are installed or
ready. Organize the manual by goals, participants, semantic actions, proof,
recovery, and authority checkpoints. Avoid volatile selectors and pixel
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

Registry provider selectors can be symbolic links to a product's current
content-addressed release. Chancery canonicalizes each once per invocation.
CLI validation reads `provider.json` and its indexed entries and manuals.
Indexed paths must stay beneath the resolved root and contain no symbolic links.
Validation rejects indexed non-files, invalid UTF-8, oversized inputs,
unsupported schemas, duplicate entry IDs, dependency cycles, and impossible
version ranges. Unindexed objects do not affect CLI validation.

Before hashing and staging, product packaging and deployment require the entire
published bundle tree to contain only regular files and directories.

Validation treats every invocation string as inert text. A bundle cannot cause
Chancery to run a command, probe a service, open a network connection, or call
a model.
