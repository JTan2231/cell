# Resolve an installed outward promise

First use `chancery list` to compare the complete catalog with the intended
outcome. Read every plausible entry with `chancery show ENTRY_ID`. After
selecting one exact entry, resolve its outward promise:

```sh
/Users/joey/.local/bin/chancery resolve ENTRY_ID
```

Chancery does not accept natural-language request text and does not rank
candidates. Extra positional text is a usage error. Resolution also does not
execute the entry, probe readiness, infer authorization, retain the request,
or prove domain success.

## What the dossier contains

The root dossier contains provider identity, release, schema, and promise scope.
It includes the complete entry and manual, normalized facets, direct dependency
status, and readiness. The basis identifies `provider.json`, the entry, and its
manual, with SHA-256 digests of the exact UTF-8 bytes read.

The dependency closure contains the same dossier for each installed transitive
`dependencies` contract, once, in stable entry-ID order. These edges describe
documentation compatibility. They do not establish runtime calls, data flow,
authority transfer, or readiness dependencies. Separate claims describe
substantive reliances. A declared cross-system reliance without a dedicated
versioned contract appears as a gap.

Each provider scope says what the provider is authoritative for, what it is
not authoritative for, and the meaningful surface class within which its
inventory is complete or partial. The provider names that surface class
independently of the entry index.

Existing entry fields provide applicability, outcome, supported interfaces,
effects, authority, success evidence, failure and recovery, privacy,
documentation dependencies, and exclusions. Schema-3 entries may add the
normalized boundary facets that were previously only prose:

- consumers and preconditions;
- inputs, outputs, and data semantics;
- identity and units;
- selected-record coverage and observation times (`completeness_and_freshness`);
- access;
- lifecycle and consistency;
- operational limits;
- compatibility and evolution; and
- substantive reliances.

Each normalized claim has `declared`, `unsupported`, `unspecified`, or
`not_applicable` status. A mixed facet preserves all statuses. Missing facets
in older or partially onboarded entries are `undeclared`. Unsupported and
not-applicable claims define boundaries. Unspecified claims appear as gaps
without making a fully authored declaration structurally incomplete.

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

## Output selection

Resolve returns the root and transitive dependency dossiers, exact bases,
requirements, declaration and closure status, readiness, gaps, and issues.
Human output puts outcome and gaps first. `--summary` returns outcome,
requirements, status, readiness, gaps, and issues with the same exit semantics.
JSON output uses schema 3.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.
