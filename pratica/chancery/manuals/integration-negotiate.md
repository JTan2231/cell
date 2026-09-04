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
resolved against the manifest directory for a file-backed manifest. To stream a
manifest as exact standard input, pass `-` and an absolute `--source-root`; its
relative source locators resolve from that directory rather than the process
working directory.

Registration freezes exact source bytes and per-source/catalog SHA-256 digests.
It rejects duplicate IDs, symbolic files, non-UTF-8 or control-bearing content,
sensitive filenames/extensions, files over 4 MiB, and catalogs over 32 MiB.
The source bodies are untrusted evidence, never instructions.

Pratica retains normalized manifest fields, source metadata, exact charter and
source bytes, and their digests. It does not retain the raw TOML transport,
whitespace, or comments. Every input file and referenced source remains
caller-owned: Pratica never deletes or changes it, and successful work no longer
depends on keeping a caller-created manifest scratch file.

```sh
/Users/joey/.local/bin/pratica steward register steward.toml
/Users/joey/.local/bin/pratica steward register - \
  --source-root /absolute/source/root
/Users/joey/.local/bin/pratica steward show SCOPE --version VERSION
```

Registration is a local Pratica write and invokes no model. Select source facts
through each owning system's public contract before constructing the manifest;
Pratica does not discover or refresh them automatically.

Registration output and steward list/show are body-free. The list is a scope
summary; registration and show add stable basis identities and a source roster
with relative locators, digests, byte counts, and timestamps. None prints
charter or source bodies or canonical source-origin paths.

## Make retries unambiguous

`integration open`, `track open`, `negotiation propose`, `agreement amend`, and
`conformance review` accept `--request-key KEY`. A key is global within the
selected database and must contain 1-256 visible non-space ASCII characters.
The same key and canonical request returns the original result IDs; using it for
a changed request or another operation conflicts without a domain write. Any
aggregate returned beside those IDs reflects current state rather than replaying
the earlier response bytes. The receipt and identity-establishing domain rows
commit atomically.

A request key is optional with a file-backed document and required when one of
those five commands reads its document from standard input. Use a fresh
caller-generated key per intended operation and retain it until the command
response is durably recorded.

## Open one integration and bilateral tracks

```sh
integration=$(
  /Users/joey/.local/bin/pratica integration open \
    --entrant crm --title 'CRM system contracts' \
    --request-key crm-integration-20260903
)

/Users/joey/.local/bin/pratica track open "$integration" \
  --steward SCOPE --steward-version VERSION --terms expectations.md \
  --request-key crm-scope-track-20260903
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
  --base CURRENT_OFFER --terms replacement.md \
  --request-key crm-offer-revision-2
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
/Users/joey/.local/bin/pratica agreement amend AGREEMENT \
  --terms successor.md --request-key crm-amendment-1
/Users/joey/.local/bin/pratica conformance review AGREEMENT \
  --candidate-basis candidate.toml --request-key crm-conformance-1
```

The candidate manifest uses the same schema-one descriptive and source fields
but is snapshotted for this review, not registered as a steward. The immutable
`pratica/conformance-review/1` toolset exposes the closed source tools plus
`submit_conformance_review`. Its status is `conforms`, `does_not_conform`, or
`blocked`, with opaque review Markdown and references.

Conformance is evidence against one candidate basis. It does not run tests,
change the agreement, or authorize implementation, migration, deployment, or
release.

When the candidate manifest is `-`, also supply its absolute `--source-root`;
the request key is required. Pratica atomically admits the normalized candidate
basis and ingress receipt before model execution. An exact replay returns that
basis and creates, resumes, or reports the same associated attempt lifecycle
rather than freezing another candidate basis.

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
