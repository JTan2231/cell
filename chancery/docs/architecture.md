# Architecture

Chancery reads installed provider bundles without retaining catalog state. Its separate usage journal records command invocations. It lists
installed capabilities and operations and presents their contracts. For one
selected entry, it assembles provider scope, normalized claims, transitive
dependency contracts, exact source references, and unresolved gaps into a
deterministic dossier.

A product release publishes documentation. Chancery lists it. The interactive
agent compares the user's request with the catalog, then decides whether and
how to invoke the documented interface.

```text
product release -- publishes --> provider bundle -- read by --> Chancery
operation bundle -- references --> capability IDs
Codex -- list/show --> Chancery -- resolve exact ID --> promise dossier
Codex -- then separately invokes ---------------------> product or UI
```

Product CLIs import Chancery's usage library for best-effort invocation recording. Catalog discovery is not an execution dependency. Chancery never submits a Nucleus
job, calls a model, opens a network connection, executes a documented
interface, grants authority, or treats runtime readiness as established.

A malformed provider is excluded without hiding valid providers. Any future
index is disposable and derived; provider bundles remain authoritative.

## Authorities

| Concern | Authority |
| --- | --- |
| Supported product outcome, effects, privacy, invocation, and domain success | Owning product and its versioned provider bundle |
| Product jurisdiction, scoped inventory completeness, normalized boundary claims, and substantive reliances | Owning product and its versioned provider bundle |
| Bundle schema, structural validity, complete catalog enumeration, exact-byte basis, dependency closure, facet classification, and presentation | Chancery |
| Whether an installed service, account, UI, credential, or data store is ready now | Represented product or interactive session |
| Whether a requested mutation is authorized | User request plus the represented product's contract |
| Cross-capability choreography | The installed operation manual; each participant keeps its own domain authority |
| Product implementation and release | Owning product |

Chancery presents the product-owned contracts. Runtime readiness is reported
through the represented product's operating interface.

## One-way intersystem dependencies

Product source owns its provider bundle. Product CI can invoke `chancery
validate`. Packaging copies the unchanged bundle into the release and publishes
one atomic selector in the Chancery registry. These actions add no runtime
dependency on Chancery.

Catalog queries read provider files. CLI dispatch separately attempts a usage-journal append. It does not call Nucleus,
Todo, Annals, Codex, a skill, a browser, or computer use. After reading
a contract, the interactive caller may use an interface named by the contract;
that is a separate action with its own authorization and failure semantics.

Dependencies between entries identify required documentation contracts. For
example, a requester can require version 1 of the Nucleus execution contract.
Each dependency requires an installed contract with a permitted version. That
contract must also have compatible dependencies.

If a dependency is unavailable, Chancery marks dependent entries as unavailable.
Those entries remain in the catalog. Chancery reads contract files for these
checks. It does not test the Nucleus daemon.

Substantive reliance is separate. A schema-3 entry can state that its outcome
relies on another system's data, control, authority, readiness, or an external
source. It can bind that reliance to a versioned dependency contract. Chancery
does not infer runtime or data lineage from the dependency graph. A declared
reliance without a dedicated contract remains a visible gap.

## Provider scope and normalized promises

Schema-3 providers publish a promise scope beside their identity and entry
index. It says what the product is and is not authoritative for, the meaningful
class of public outcomes its inventory covers, whether that inventory is
complete or partial within that class, and the shared access, privacy,
retention, compatibility, retirement, and operating limits that qualify all
entries. This is a small outward-facing scope declaration, not a replacement
for capability contracts, product documentation, or implementation proof.

Entries remain the unit of reliance. Existing required fields state
applicability, outcome, interface, effects, authority, success, failure and
recovery, privacy, dependencies, and exclusions. An optional schema-3
declaration normalizes the facets that otherwise tend to remain prose:
consumers, preconditions, inputs, outputs, data semantics, identity and units,
selected-record coverage and observation times, access, lifecycle and
consistency, operational limits, compatibility and evolution, and substantive
reliances. The `completeness_and_freshness` facet names the selected records or
operation whose scope and timestamps it describes.

Each normalized claim has `declared`, `unsupported`, `unspecified`, or
`not_applicable` status. The resolver marks omissions as `undeclared`. A facet
can contain mixed claims. For example, it can declare transactional visibility
while leaving its wall-clock bound unspecified.

Schemas 1 and 2 remain readable during rollout. They have no provider scope or
normalized declarations, so exact-ID resolution returns their complete
existing contracts alongside explicit declaration gaps instead of rejecting
the whole catalog.

## Exact-ID resolution

`resolve ID` is a projection over one already selected entry. It does not
accept free-form intent, rank candidates, or replace `list` and `show` during
semantic selection. The result includes:

- the root provider scope, complete entry and manual, facet coverage, direct
  dependency status, and separate availability, compatibility, and readiness;
- every installed transitive documentation dependency exactly once in stable
  entry-ID order;
- optional root contract-version and positive-facet requirement results;
- explicit undeclared, unspecified, partial-inventory, uncontracted-reliance,
  and dependency gaps; and
- paths and SHA-256 digests for the exact raw UTF-8 provider manifest, entry,
  and manual bytes used as the basis.

Documentary success is `resolved_not_ready`: the promise resolves, while live
readiness remains `not_checked` or `session_dependent`. Incomplete declarations
and unavailable dependency closures remain inspectable but return nonzero so a
consumer cannot silently treat them as complete. Resolution never reads a
database schema or implementation to fill a publisher gap.

## Capability and operation documents

A capability describes one supported outcome owned by one product. Modes keep
audiences distinct:

- `use` is ordinary outcome-oriented work;
- `operate` is installation, readiness, administration, or recovery; and
- `develop` changes an implementation or integration.

An operation describes how to coordinate capabilities or changing interactive
interfaces. It records semantic steps, checkpoints, authorization, adaptation,
recovery, and stop conditions without executing a workflow. A computer-use
operation can remain useful as an interface changes. Stable CLI invocations
remain in the owning capability contracts.

## Failure isolation

The registry has no database or daemon. Each invocation resolves provider
selectors to canonical bundles, validates indexed files, and creates its view
and requested dossier in memory. It then prints the result and exits. Invalid
providers are reported and excluded as units. Filesystem order does not resolve
duplicate global entry IDs. Missing or incompatible dependencies affect the
referenced entries without corrupting their bundles.

Chancery installation preserves product-owned provider selectors. Each product
release includes its bundle in its own content hash; its installer advances or
rolls back the release and selector coherently. This makes repair local to the
owning product and avoids a central mutable catalog authority.

## Compatible deployment order

When changing the schema or adding providers, use:

1. deploy a reader that accepts schema 3 while retaining schema-1 and schema-2
   support;
2. deploy schema-3 product bundles and their selectors;
3. deploy cross-product operation bundles after all required capabilities;
4. change the global agent bootstrap last.

Removing or incompatibly changing a contract reverses that logic: update the
agent bootstrap and dependents first, then remove the old provider contract
only when no installed operation requires it.

Output schema 3 separates selection, operation, and declaration detail: `list`
uses shared defaults and exceptions, `show` reads the self-contained authored
manual once, `show --full` preserves authoring fields, and `resolve --summary`
selects outcome and gaps from the same complete resolution. These projections
never alter stored bundle bytes or infer missing promises.

## Rust callers

Rust callers use `chancery::api::Client` for typed list, show, resolve, doctor,
and validate operations. The caller selects an executable and registry.
Reports distinguish unresolved or invalid domain results from transport errors.

The Rust library exposes provider-owned bundle documents and CLI output types
through `chancery::api`. `ProviderManifest::decode` and `EntryDocument::decode`
use the same codecs as the CLI, including legacy schema handling. Decoding a
document is separate from full bundle validation.

`ProviderIntroduction` and `EntryIntroduction` read the identity and
indexed-manual fields that Usher needs. They ignore other fields and do not
evaluate promises, dependencies, or full bundle validity. Usher owns membership
policy. `Output<T>` and the command result types define the JSON output that
the CLI serializes.
