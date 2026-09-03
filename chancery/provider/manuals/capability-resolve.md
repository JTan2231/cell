# Resolve an installed outward promise

Resolution begins after semantic discovery. Use `chancery list`, compare the
complete catalog with the intended outcome, and read every plausible entry
with `chancery show ENTRY_ID`. Once one exact entry is selected, assemble its
outward-promise dossier:

```sh
/Users/joey/.local/bin/chancery resolve ENTRY_ID
```

Chancery does not accept natural-language request text and does not rank
candidates. Extra positional text is a usage error. Resolution also does not
execute the entry, probe readiness, infer authorization, retain the request,
or prove domain success.

## What the dossier contains

The root dossier contains the owning provider identity and release, provider
schema and promise scope, complete entry document and manual, normalized facet
coverage, direct dependency status, and readiness classification. The basis
names `provider.json`, the indexed entry, and its manual and gives a SHA-256
digest of the exact UTF-8 bytes read for each.

The dependency closure contains the same complete dossier for every installed
transitive `dependencies` contract, once, in stable entry-ID order. Those
edges mean documentation-contract compatibility only. They do not prove a
runtime call, data flow, authority transfer, or readiness dependency.
Substantive reliances are separate normalized claims. A declared cross-system
reliance with no dedicated versioned contract is reported as a gap.

Each provider scope says what the provider is authoritative for, what it is
not authoritative for, and the meaningful surface class within which its
inventory is complete or partial. Completeness does not mean “whatever this
bundle happened to index,” nor does it mean every behavior of every installed
product is published.

Existing entry fields provide applicability, outcome, supported interfaces,
effects, authority, success evidence, failure and recovery, privacy,
documentation dependencies, and exclusions. Schema-3 entries may add the
normalized boundary facets that were previously only prose:

- consumers and preconditions;
- inputs, outputs, and data semantics;
- identity and units;
- completeness and freshness;
- access;
- lifecycle and consistency;
- operational limits;
- compatibility and evolution; and
- substantive reliances.

Every normalized claim is explicitly `declared`, `unsupported`, `unspecified`,
or `not_applicable`. A mixed facet preserves all statuses. An older or
partially onboarded entry has `undeclared` coverage instead of a guessed
meaning. Explicitly unsupported and not-applicable claims are boundaries, not
silence. Explicitly unspecified claims are reported as gaps, but they do not
make a fully authored declaration structurally incomplete.

## Resolution status

`resolved_not_ready` means the documentary promise, scoped inventory, and
installed dependency closure resolve. Readiness still remains `not_checked` or
`session_dependent`; check it separately through the owning product only when
the user asks to use the capability.

`incomplete_declaration` means a provider scope, normalized entry declaration,
dedicated reliance contract, or caller-required positive facet is missing.
`dependency_unavailable` means a declared documentation dependency is missing,
incompatible, cyclic, or transitively unavailable. `contract_incompatible`
means the root does not meet caller-supplied version bounds. These results
return the inspectable dossier and gaps with a nonzero status so automation
cannot silently treat them as a complete promise.

Require one or more positive facets when a design has a specific reliance:

```sh
/Users/joey/.local/bin/chancery resolve ENTRY_ID \
  --min-contract 1 \
  --max-contract-exclusive 2 \
  --require data_semantics \
  --require completeness_and_freshness
```

Requirements are satisfied only by at least one positive `declared` claim in
that root facet. Unsupported, unspecified, not-applicable, and undeclared
facets remain unsatisfied. A reported gap should be taken back to the owning
product contract; do not fill it by reading a database schema or implementation
code and calling the inference a promise.
