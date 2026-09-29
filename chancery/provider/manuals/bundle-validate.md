# Provider bundles and validation

One provider bundle has this shape:

```text
provider.json
overview.md              # optional, schema 4
entries/
  ENTRY.json
manuals/
  ENTRY.md
```

`provider.json` indexes every entry and an optional product overview. Chancery
reads indexed files only.
Provider selectors in the registry can be symbolic links. Indexed paths cannot
contain symbolic links and must stay beneath the fixed bundle root. Product
packaging checks the entire bundle tree before publication.

Schema version, provider release, and each entry contract version are
independent. Dependencies name stable entry IDs and integer contract-version
bounds. Operations additionally declare session surfaces whose live
availability must be checked by the interactive agent.

## Provider file

New and updated providers can use schema version 4. Schemas 1 through 3 remain
readable; only schema 4 can index a product overview. `provider.json` is UTF-8
JSON with no unknown fields:

```json
{
  "schema_version": 4,
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
  "overview": "overview.md",
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

`overview` is an optional relative path to a nonempty UTF-8 Markdown document.
The document explains the product, its authority boundary, and how its features
and operations fit together. It has no entry ID or contract version. The
provider release selects its bytes. `chancery product PROVIDER_ID` presents it
with the provider identity and complete entry inventory. When it is absent,
the product view reports `overview_status: not_published`; Chancery does not
infer an overview from entry summaries or a source checkout.

The overview has the same path, file-size, UTF-8, and control-character checks
as an entry manual. An invalid indexed overview invalidates the provider.
Schemas 1 through 3 reject `overview`. Schema 4 retains schema-3 scope and entry
rules; it requires `promise_scope` and permits the same optional `promise`.

## Feature documents

A feature is a coherent product capability, represented by one `capability`
entry and its manual. No separate feature kind or duplicate capability record
is required. The owning product keeps the full explanation in its provider
bundle: purpose, boundary, supported interfaces, behavior, data meaning,
permissions, lifecycle, recovery, compatibility, and material limits.

Use stable Chancery entry IDs to refer to other installed documents. Declare a
version-bounded `dependencies` edge when the document requires another
contract. `resolve` then reads that complete dependency contract. Related
references in Markdown are navigation only: they do not affect compatibility
or resolution and need not form an acyclic graph. Required dependencies must
remain acyclic. Do not use repository links as a substitute for installed
feature content. Chancery does not expand Markdown includes or section links.

A procedure keeps its prerequisites, exact actions, consequential effects,
authority, success evidence, recovery, privacy, exclusions, and stop conditions
in its own manual. Its complete operating essentials must be usable through
`show`. Put the detailed behavior that the procedure relies on in the required
feature contracts and read the assembled explanation through `resolve`.

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
| `promise` | Optional schema-3/4 normalized outward-boundary declaration; complete when present |
| `manual` | Indexed, nonempty, UTF-8 detailed Markdown beneath the bundle |

Titles and summaries support discovery. They must distinguish entries by their
intended results. An internal command name or generic noun is insufficient.
Chancery displays them verbatim. The interactive agent compares their meaning
with the user's request.

Collections required by the selected entry kind must not be empty. Text values
must be nonblank. Collection text values and interface labels are unique after
Unicode NFKC normalization and lowercase conversion. Dependency IDs and indexed
paths must be unique. Unknown fields are rejected so that misspellings cannot
silently weaken a contract.

Provider IDs start with a lowercase ASCII letter and continue with lowercase
ASCII letters, digits, or dashes. Entry IDs have at least two dot-separated
segments with the same rule and begin with their provider ID. Entry paths have
exactly the form `entries/NAME.json`; manuals have exactly the form
`manuals/NAME.md`. Other indexed paths are nonempty relative paths with only
normal components. Absolute roots and parent components are rejected.
Contract versions and dependency minima are positive integers. Each exclusive
maximum must exceed its minimum. JSON text fields reject control characters.
Manual and overview Markdown permit line feed and tab, but reject other control
characters.

## Normalized promise declaration

The optional schema-3/4 `promise` object is all-or-nothing. When present, each of
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

## Earlier schemas

The reader temporarily accepts provider schema v1 so independently deployed
products can migrate after Chancery. Its entry documents may contain the old
`routable` and `routing` fields. Chancery ignores those two fields completely:
they do not filter the catalog, influence selection, or appear in list/show
output. All other v1 fields retain the same validation rules.

Schema v2 removes both fields. A v2 entry containing either one is invalid.
Schemas 1 and 2 cannot contain `promise_scope` or `promise`; exact-ID
resolution preserves their full existing documents and reports provider scope
and normalized facets as undeclared.

Schema 3 requires `promise_scope` and allows the optional `promise` declaration,
with no product overview. It remains valid without migration.

Deploy a reader that accepts schema 4 before publishing schema-4 bundles.
A schema-3 provider can adopt schema 4 and index an overview without changing
its entry IDs or compatible contract versions. Providers migrating from schema
1 or 2 also add a complete `promise_scope` and normalize entries deliberately.
Entry `promise` remains optional so one provider can onboard in bounded steps.
Unknown fields and incomplete scope or promise objects are invalid.

## Validation and security

Registry provider selectors can be symbolic links to a product's current
content-addressed release. Chancery canonicalizes each once per invocation.
CLI validation reads `provider.json`, its optional indexed overview, and its
indexed entries and manuals.
Indexed paths must stay beneath the resolved root and contain no symbolic links.
Validation rejects indexed non-files, invalid UTF-8, oversized inputs,
unsupported schemas, duplicate entry IDs, dependency cycles, and impossible
version ranges. Unindexed objects do not affect CLI validation.

Before hashing and staging, product packaging and deployment require the entire
published bundle tree to contain only regular files and directories.

Validation treats every invocation string as inert text. A bundle cannot cause
Chancery to run a command, probe a service, open a network connection, or call
a model.

## Standalone validation

Product CI validates a standalone source bundle without an installed registry:

```text
$ chancery validate /absolute/path/to/example/provider
PASS  Example 2.4.1
Bundle: /absolute/path/to/example/provider
Entries: 1
External dependencies: not checked
```

Structural failures print every detected issue and exit 1. Dependencies that
name another entry in the same bundle are checked for contract-version
compatibility and cycles. Cross-provider dependencies are deliberately
reported as `not_checked`; installed compatibility belongs to `doctor`.

The current provider schema is version 4. The reader also accepts schemas 1,
2, and 3. It discards obsolete `routable` and `routing` metadata only for
schema 1 and presents the same catalog and `show` shape. Schemas 3 and 4 require
a provider promise scope. Schema 4 additionally permits an indexed product
overview. Earlier bundles remain valid without one; older entries resolve
with explicit normalized gaps.


## Validation bounds and result

| Selected input | Maximum |
| --- | ---: |
| Registry provider selectors | 1,024 |
| Indexed entries in one provider | 4,096 |
| `provider.json` or one entry JSON | 1,048,576 bytes |
| One manual or overview Markdown | 4,194,304 bytes |
| One validated text field | 65,536 UTF-8 bytes |

The standalone validator reads the selected bundle and retains no catalog or
provider state. CLI dispatch can separately append command-usage metadata;
read `chancery show chancery.usage.record` for that boundary. A valid bundle
returns exit 0. Structural or internal-dependency failure returns the full
report with exit 1. CLI syntax errors return exit 2. Validation alone does not
prove prose completeness, installation, live readiness, authorization, or the
represented product's implementation conformance.

JSON uses Chancery output schema 3. `ValidateResult` has `valid`, optional
`provider`, `bundle`, optional `entries`, `external_dependencies`, and `issues`.
`external_dependencies` is `not_checked` for external documentation dependencies. An
invalid report retains its data with envelope `ok: false`. Read
`chancery show chancery.directory.discover` for the common CLI envelope and
provider-owned Rust client. The client method is `Client::validate(&Path)`.

## Release publication guarantees

The owning product stages its unchanged bundle under
`share/chancery/PROVIDER_ID` inside its content-addressed release. Bundle bytes
participate in the release identity and integrity manifest. Its single selector
under `~/Library/Application Support/Chancery/providers/PROVIDER_ID` follows
that product's `current` release. A failed upgrade or rollback restores the
program and documentation together. The owning installer must reject a
pre-existing selector owned by someone else.

A selector can precede installation of the Chancery CLI. Publication is a
packaging action. Product runtime does not invoke the catalog, and product
installation must remain useful without the reader. Chancery upgrades preserve
other product selectors. A combined release can carry independently versioned
providers, as Annals does. Earlier shared installations used `share/chancery`
directly; the Rust installer verifies that legacy layout when it admits an
existing installation or recovers a retained release.

Deploy a compatible reader before changing the provider schema. Publish the
required provider releases before cross-product operations or bootstrap
instructions rely on them. Update consumers before removing incompatible
contracts. No universal deprecation window or legacy-schema retirement date is
promised. Read `chancery show chancery.provider.publish` for the publication
procedure and `chancery show chancery.installation.operate` for reader
installation and recovery. Do not edit an installed content-addressed bundle
or point an installed selector at a source checkout.
