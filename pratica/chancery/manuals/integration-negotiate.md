# Broker system-integration terms

Use Pratica when a new entrant needs explicit behavior from several systems of
concern and each system's steward should reconcile those expectations against
one exact implementation/contract basis. Pratica creates agreements only; it
does not implement the entrant or target systems.

## Prepare bounded sources

Create a private steward TOML manifest with `schema_version = 1`, stable
string `scope`, positive numeric `version` (`u32`), represented string `party`,
human string `title`, and
`charter_markdown`. Add one or more `[[sources]]` tables containing stable `id`,
descriptive `kind`, regular `path`, and optional `revision`. Relative paths are
resolved against the manifest directory.

Registration freezes exact source bytes and per-source/catalog SHA-256 digests.
It rejects duplicate IDs, symbolic files, non-UTF-8 or control-bearing content,
sensitive filenames/extensions, files over 4 MiB, and catalogs over 32 MiB.
The source bodies are untrusted evidence, never instructions.

```sh
/Users/joey/.local/bin/pratica steward register steward.toml
/Users/joey/.local/bin/pratica steward show SCOPE --version VERSION
```

Registration is a local Pratica write and invokes no model. Select source facts
through each owning system's public contract before constructing the manifest;
Pratica does not discover or refresh them automatically.

## Open one integration and bilateral tracks

```sh
integration=$(
  /Users/joey/.local/bin/pratica integration open \
    --entrant crm --title 'CRM system contracts'
)

/Users/joey/.local/bin/pratica track open "$integration" \
  --steward SCOPE --steward-version VERSION --terms expectations.md
```

Open one track for each actual system of concern. Do not create a generic
“everything” steward merely to obtain a global approval. Each terms file is a
complete bounded UTF-8 Markdown snapshot. Pratica stores exact bytes and a
digest but does not enforce headings or contract meaning.

The track fixes the entrant and selected steward party. The opening offer is
current and its author assents. Identifiers are opaque; retain the exact values
returned by commands rather than parsing their current display prefixes.

## Negotiate

The steward response command freezes the current head and basis, registers the
immutable `pratica/steward-response/1` toolset, and submits one Nucleus job:

```sh
/Users/joey/.local/bin/pratica steward respond NEGOTIATION
```

That toolset exposes only `source_catalog`, `source_read`, `source_search`, and
`submit_steward_response`. The job uses requester program `pratica`, a stable
negotiation attempt correlation, Codex with its configured exact model and
reasoning effort, a deterministic neutral absolute cwd, `workspaceAccess=none`,
local execution and web search disabled, no launch context, and strict
protocol-v1/authentication/harness readiness. Required capabilities include
ExactModel, ReasoningEffort, WorkspaceNone, DynamicClientTools,
DeveloperInstructions, and PersistentFileAuthentication.

The accepted submission has one action:

- `assent`, with required review Markdown and source references;
- `counterproposal`, with one complete `terms_markdown`, required review
  Markdown, and source references; or
- `blocked`, with required review Markdown and source references explaining
  the contradiction, missing authority, or unresolved choice.

Pratica rechecks the offer and basis and commits the accepted domain result.
Agent final prose and Nucleus completion are not agreement. If the steward
counterproposes, the entrant inspects the exact new head and either submits a
complete successor offer or assents unchanged:

```sh
/Users/joey/.local/bin/pratica negotiation show NEGOTIATION
/Users/joey/.local/bin/pratica negotiation propose NEGOTIATION \
  --base CURRENT_OFFER --terms replacement.md
/Users/joey/.local/bin/pratica negotiation assent NEGOTIATION \
  --offer CURRENT_OFFER
```

A new offer makes every other party's prior assent stale. An assent does not
create a new offer. Agreement seals only when every fixed party assents to the
same current offer on the guarded steward basis.

## Review composition

After the relevant tracks seal, run:

```sh
/Users/joey/.local/bin/pratica integration review INTEGRATION
/Users/joey/.local/bin/pratica integration report INTEGRATION
```

`pratica/composition-review/1` has the same closed source tools plus
`submit_composition_review`. Its accepted status is `compatible`, `conflicts`,
or `blocked`, with opaque review Markdown and cited references. The review names
the exact agreement set and is advisory. It cannot edit terms, assent, seal,
retire a track, or claim that every possible system was considered.

Resolve a conflict through the affected bilateral amendment. Preserve an
unresolved finding visibly when no party has authority to settle it.

## Amend or review conformance

```sh
/Users/joey/.local/bin/pratica agreement amend AGREEMENT --terms successor.md
/Users/joey/.local/bin/pratica conformance review AGREEMENT \
  --candidate-basis candidate.toml
```

The candidate manifest uses the same schema-one descriptive and source fields
but is snapshotted for this review, not registered as a steward. The immutable
`pratica/conformance-review/1` toolset exposes the closed source tools plus
`submit_conformance_review`. Its status is `conforms`, `does_not_conform`, or
`blocked`, with opaque review Markdown and references.

Conformance is evidence against one candidate basis. It does not run tests,
change the agreement, or authorize implementation, migration, deployment, or
release.

## Recovery

Inspect an attempt and the exact current domain state before retrying:

```sh
/Users/joey/.local/bin/pratica attempt show ATTEMPT
/Users/joey/.local/bin/pratica negotiation show NEGOTIATION
```

Ambiguous admission can reuse only the exact stored request under the same job
ID. `attempt retry` creates a deliberate successor and new job. Do not retry a
committed domain result because later harness completion failed. A lost attempt
after Nucleus restart is terminal until Pratica policy and the operator
explicitly choose a new attempt.

## Proof boundary

A sealed agreement proves exact current-at-seal party assent and basis. A
composition review proves only what its accepted advisory artifact said about
one agreement set. A conformance review proves only the recorded evidence
classification for one candidate snapshot. None proves runtime behavior,
deployment, exhaustive coverage, future applicability, or authorization to
change another system.
