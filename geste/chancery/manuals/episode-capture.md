# Record a manual Geste episode

Capture one deliberately bounded historical case only after consulting the
relevant source products through their own public contracts. Geste v0.1 does
not resolve or verify an upstream reference. The caller supplies stable source
anchors and authors the episode interpretation; each source remains
authoritative for its own record.

The input is strict UTF-8 JSON no larger than 256 KiB and has
`"schema_version": 1`. `INPUT` is `-` for bounded standard input or a regular,
non-symbolic file; both are read through a maximum-plus-one bounded stream.
Unknown fields are rejected. A create and a revision use the same
complete-snapshot body with these top-level members:

- `title`, `shape`, `basis_cutoff_at`, `recorded_by`, `situation`, `response`,
  `outcome`, and `applicability`;
- ordered `actions`, `lessons`, and `settlements`;
- normalized `tags`, explicit `gaps`, `sources`, and `related_episodes`.

`outcome` is exactly `{status, summary}` and its status is `solved`, `partial`,
`failed`, or `unknown`. A settlement is exactly `{id, statement, status, gap}`
with status `verified` or `unverified`. A verified settlement needs at least
one supporting source whose system is `decisions`, kind is `lifecycle_event`,
and role is `authority`, and its `gap` must be null. An unverified settlement
must name an existing `gap:N`. Other prose is Geste-authored episode
interpretation, not enacted authority. Nullable settlement `gap` and source
`revision` and `digest` members remain required even when their value is null.

A source contains `id`, lowercase `system`, source-owned `kind` and
`reference`, `revision` as a string or null, `digest` as 64 lowercase
hexadecimal SHA-256 characters or null, `observed_at`, `role`, `label`, and
`supports`. Allowed roles are `authority`, `context`, `evidence`, `effect`,
`procedure`, and `outcome`. Observation may not be later than
`basis_cutoff_at`. A source with neither revision nor digest is locator-only;
reports warn that it cannot verify mutable upstream state.

Revision meaning remains source-owned. A Git tag name alone is mutable and
belongs in `reference`; use a full commit, tree, or tag-object ID as the Git
`revision`, or pair the tag reference with a source digest. With a null digest,
Geste rejects a non-null Git revision unless it is a full 40- or 64-character
lowercase hexadecimal object ID.

Support targets are `shape`, `situation`, `response`, `outcome`,
`applicability`, `action:N`, `lesson:N`, `settlement:ID`, or `gap:N`, with
one-based indices. A related episode is exactly `{episode, revision, relation}`
and freezes an existing revision. Relations are `builds_on`, `similar_to`,
`contrasts_with`, and `supersedes`; duplicate and self-links are rejected.

Create a new identity with:

```sh
/Users/joey/.local/bin/geste episode create /private/path/episode.json
```

Append later understanding without rewriting history:

```sh
/Users/joey/.local/bin/geste episode revise e12 \
  /private/path/episode-v2.json --base 1
```

`--base` is the current head revision observed by the caller. The append
transaction rechecks it and returns `stale_revision` without writing if another
revision is already current. Inspect that head, reconcile the new complete
snapshot, and retry deliberately. Nothing omitted from a revision is inherited
from the preceding revision; omissions affect only the appended snapshot and
older revisions remain unchanged.

Validation requires bounded nonblank text, unique normalized tags and source
IDs, valid support targets, at least one source, source times within the basis,
and existing exact related revisions. Title and recorded-by are at most 200
Unicode scalar values; shape is at most 1,000; situation, response, outcome,
and applicability are at most 4,000 each. Collections contain at most 128
members; tags contain at most 64 values of at most 64 scalars; remaining member
text and labels are at most 2,000 scalars.

Create and revise each commit one foreign-key-checked SQLite transaction whose
revision seal is written last. Sealing triggers reject later child inserts;
history updates and deletes are also refused. Geste stores the SHA-256 digest
of the exact submitted bytes and normalized fields, not a second request-file
copy or an upstream body. A validation, unsupported schema, missing relation,
or stale-base error leaves no partial revision.

The database must already have been created by the separate visible
`geste init` operation. Capture makes no model, network, Chancery, or
source-product call. Protect both request files and the private database; they
may contain sensitive process accounts, decisions, labels, identities, and
outcomes.
