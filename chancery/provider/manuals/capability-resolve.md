# Resolve an installed outward promise

Read the complete installed catalog and every plausible page first.
Read `chancery show chancery.directory.discover` for discovery. After selecting
one exact entry, resolve its complete
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
the resolver does not infer runtime calls or data flow from them. Related
Chancery references in Markdown provide navigation only and do not enter the
dependency closure. Resolution does not expand sections or include a separate
product overview body.

`resolved_not_ready` returns exit code 0. Other resolution statuses return the
full dossier on stdout and exit code 1. Resolution does not test readiness,
execute an interface, or grant authorization.


## Invocation, identity, and consistency

```text
chancery [--registry PATH] [--json] resolve ID [--min-contract VERSION] [--max-contract-exclusive VERSION] [--require FACET]... [--summary]
```

`--registry` overrides `CHANCERY_REGISTRY`; the default is
`~/Library/Application Support/Chancery/providers`. Bounds are positive
integers, and an exclusive maximum must exceed a supplied minimum. The query
reads the same fixed installed registry view as discovery. It canonicalizes
each provider selector once, calculates the finite dependency closure, prints
the result, and retains no dossier or request state. The closure excludes the
root, includes incompatible installed contracts for inspection, and terminates
on cycles. Provider release, provider schema version, entry contract version,
and output schema version remain separate.

Basis paths are relative to the fixed bundle. Digests are lowercase SHA-256 of
the exact raw UTF-8 `provider.json`, indexed entry JSON, and manual bytes.
Neither a related Markdown reference nor the separate overview body enters
that dependency closure. Substantive data, control, authority, readiness, and
external reliances are declared separately. Chancery cannot infer a promise
from code, a schema, tests, an omitted field, or a dependency edge.

## Typed results

JSON uses the common Chancery schema-3 envelope described by
`chancery.directory.discover`. `ResolveResult` has `requested_id`, `status`,
`contract_requirement`, `facet_requirements`, `declaration_status`,
`dependency_closure_status`, `readiness`, `root`, `dependency_closure`, `gaps`,
and `issues`. `ResolveSummary` has the same outcome fields without `root` or
`dependency_closure`. `ContractRequirement` has `min_contract`,
`max_contract_exclusive`, and `satisfied`; `FacetRequirements` has `required`
and `unsatisfied`.

Each `ContractDossier` has `provider`, `provider_schema_version`,
`provider_promise_scope`, `entry`, `facet_coverage`, `availability`,
`compatibility`, `readiness`, `dependency_statuses`, `manual`, and `basis`.
`FacetCoverage` has `state` and `claim_statuses`. `ContractBasis` names
`provider_manifest`, `entry_contract`, and `manual`, each with `path` and
`sha256`. Each gap has `code`, `message`, and optional `entry` and `facet`.
Rust callers use `Client::resolve` or `Client::resolve_summary` through
`chancery::api`; unresolved dossiers remain reports with `ok: false`.

## Limits, privacy, and recovery

Resolution is bounded by the installed registry, indexed file and field
limits, and finite dependency graph. Read `chancery.bundle.validate` for the
exact bounds. No wall-clock resolution latency or future compatibility window
is promised. Provider schemas 1 through 4 remain readable; schemas 1 and 2
retain explicit missing-scope and normalized-facet gaps. No legacy-schema
retirement date is promised.

Resolution processes local installed documentation. It retains no caller
intent or dossier, performs no network or model request, probes no account or
service, and grants no represented access. Missing or excluded IDs return
`entry_not_found`; return to discovery or repair the owning provider. Preserve
unresolved dossiers for inspection and take a missing promise back to its
owner. Publish the owner-approved explanation instead of filling a gap from
implementation details. Check readiness separately through the owning product
only when actual use is intended.

CLI dispatch separately attempts a metadata-only usage append. Missing thread
attribution and internal calls are skipped; recording errors preserve the
resolution result. Read `chancery.usage.record` for this feature.
