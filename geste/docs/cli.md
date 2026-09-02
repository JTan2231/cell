# CLI contract

```text
geste [--database PATH] [--json] init
geste [--database PATH] [--json] doctor
geste [--database PATH] [--json] search QUERY [--limit N]
geste [--database PATH] [--json] episode create INPUT
geste [--database PATH] [--json] episode revise EPISODE INPUT --base N
geste [--database PATH] [--json] episode list [--limit N]
geste [--database PATH] [--json] episode show EPISODE [--at N]
geste [--database PATH] [--json] report EPISODE [--at N]
geste [--database PATH] [--json] graph EPISODE [--at N]
```

`INPUT` is a regular, non-symbolic UTF-8 JSON file or `-` for standard input.
Both paths are read through a 256-KiB-plus-one bounded stream before validation.
The selected database comes from `--database`, then nonempty
`GESTE_DATABASE`, then
`$HOME/Library/Application Support/Geste/geste.db`. No other command creates a
missing database. `--json` emits one compact success object on stdout or one
stable coded error object on stderr.

## Initialization and doctor

`init` creates schema 1 without replacing a foreign or unsupported database.
Repeating it against an existing valid Geste schema is a current-format check.

`doctor` checks only selected-database existence, schema 1, SQLite integrity,
foreign keys, and private file and parent-directory modes. It performs no
Chancery, Semantics, Decisions, Nucleus, model, network, or source readiness
check.

## Capture document

Unknown fields are rejected at every level. The maximum input is 256 KiB.

```json
{
  "schema_version": 1,
  "title": "Add an episode casebook",
  "shape": "A cross-product capability needs an explicit contract gate",
  "basis_cutoff_at": "2026-09-02T18:00:00Z",
  "recorded_by": "codex",
  "situation": "The existing systems did not retrieve process cases by shape.",
  "response": "Review the cross-product contract, then implement and deploy.",
  "outcome": {"status": "solved", "summary": "The installed casebook worked."},
  "applicability": "Use for underspecified cross-product capability work.",
  "actions": ["Reviewed the contract"],
  "lessons": ["Preserve authority and stopping conditions"],
  "settlements": [
    {
      "id": "establish-geste",
      "statement": "Create Geste as a Chancery member.",
      "status": "verified",
      "gap": null
    }
  ],
  "tags": ["process-memory"],
  "gaps": [],
  "sources": [
    {
      "id": "design-decision",
      "system": "decisions",
      "kind": "lifecycle_event",
      "reference": "de_example",
      "revision": null,
      "digest": null,
      "observed_at": "2026-09-02T18:00:00Z",
      "role": "authority",
      "label": "User settlement establishing Geste",
      "supports": ["settlement:establish-geste"]
    }
  ],
  "related_episodes": [
    {"episode": "e7", "revision": 2, "relation": "builds_on"}
  ]
}
```

Outcome status is `solved`, `partial`, `failed`, or `unknown`. Settlement
status is `verified` or `unverified`. An unverified settlement names `gap:N`;
a verified settlement sets `gap` to null and is supported by a Decisions
lifecycle authority source. The nullable `gap`, source `revision`, and source
`digest` members are required even when their value is null.

Source role is `authority`, `context`, `evidence`, `effect`, `procedure`, or
`outcome`. Revision is a string or null; digest is 64 lowercase hexadecimal
SHA-256 characters or null. Support targets are `shape`, `situation`,
`response`, `outcome`, `applicability`, `action:N`, `lesson:N`,
`settlement:ID`, or `gap:N`. Observation time may not exceed the basis cutoff.
Related relations are `builds_on`, `similar_to`, `contrasts_with`, and
`supersedes`; each freezes an existing revision and cannot point to itself.

Source revision meaning follows the owning system. A Git tag name by itself is
mutable and must stay a locator in `reference`; freeze it with a full commit,
tree, or tag-object ID in `revision`, or provide a source digest. When a Git
source has a null digest, Geste rejects any non-null revision that is not a full
40- or 64-character lowercase hexadecimal object ID.

Each capture is a complete snapshot. Omitted prior values are absent only from
the new revision. `revise` appends only when `--base` equals current HEAD.

## Search

The query becomes 1 through 16 unique terms after NFKC normalization,
lowercasing, and whitespace collapse. Every term must match. Each contributes
its best weight: exact tag 8, shape 6, title 5,
situation/response/applicability 3, and outcome/lesson 2. Matching uses
normalized substrings; outcome includes both status and summary. Results sort
by score descending and numeric episode ID ascending, and expose matched terms
and fields. A result remains a precedent candidate requiring agent judgment.
