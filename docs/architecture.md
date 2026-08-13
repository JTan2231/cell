# Annals architecture

## Boundary and ownership

Annals is one Rust executable and one SQLite database per library. A work is an
immutable source object. The corpus owns concepts. Evidence is many-to-many: a
work may support many concepts and a concept may be supported by many works.
Model runs own examinations and reconciliations, never concepts.

Only an applied reconciliation whose projected corpus differs mechanically
from its base advances the corpus. Retaining a work, reading state, running a
model, recording a reconciliation, validating, backing up, and rebuilding
search data do not advance the corpus revision. A mechanically equal
projection is retained as an interpretive record with status `recorded` and
has no commit or revision of its own.

## Liaison flow

`annals integrate` content-addresses or selects one work and reads HEAD. Before
starting a model, Annals may reuse the newest successful reconciliation for
the exact same work, base revision, prompt version, model, and reasoning
effort. `--reexamine` bypasses that reuse. A later revision or different
liaison configuration always creates a fresh examination.

For a fresh examination, Annals creates a model-run record bound to the work
and base revision and starts an isolated Codex app-server process. The
high-quality default uses `gpt-5.6-sol` with max reasoning. The low and medium
presets use `gpt-5.6-luna` and `gpt-5.6-terra`, respectively, both with medium
reasoning. An exact model override changes the model while the selected preset
continues to control reasoning effort. Annals records the resolved model and
effort, loads the installed Codex model catalog, and narrows that model to
direct tool mode. The process uses an empty temporary directory, a private
temporary Codex home, and no execution environment, and disables the shell,
web, planning, user-input, multi-agent, plugin, skill, and other built-in tool
sources.

The prompt is deliberately a pointer. It contains the work label, base
revision, and operating instructions, but not the complete work. The work is
presented through read tools as source content, never as operating
instructions.

The liaison constructs a provisional, best-current reconciliation. It does not
exclude source-grounded material because it appears familiar, minor,
speculative, redundant, obvious, or unlikely to be useful. It chooses coherent
granularity relative to the work and frozen corpus without claiming a unique,
objective, or final decomposition into semantic units. It avoids mechanical
sentence-per-concept splitting, but estimated salience or novelty is not an
inclusion test.

At thread start Annals supplies exactly six direct, session-scoped dynamic
tools through the app-server protocol:

- `work_overview()` returns byte size and a bounded Markdown-heading outline;
- `work_read(regions[])` performs bounded reads by heading path, unique quote,
  beginning/end anchor, or an exact quotation returned as a continuation;
- `work_search(queries[])` returns compact paragraph excerpts and heading paths;
- `corpus_search(queries[])` searches the frozen base revision and returns paths
  and evidence;
- `corpus_inspect(paths[])` returns topology and evidence for exact paths;
- `submit_reconciliation(reconciliation)` records the session's reconciliation.

Read and search calls accept batches. One successful `submit_reconciliation`
closes the session's sole write boundary. Failed submissions are returned as
recoverable tool errors and may be corrected. Tool arguments and results are
retained in the model-run transcript.

App-server sends each dynamic tool call back to the host, which dispatches it
to the same in-process liaison backend used by the standalone private MCP
transport. The MCP server remains a transport adapter and test surface; it is
not attached to the Codex liaison, so Codex's MCP resource tools never enter
the model-visible inventory.

The model's final response is diagnostic only. `integrate` succeeds from the
recorded `submit_reconciliation` side effect. If the process fails after
recording a valid reconciliation, that reconciliation remains the result; if
the process exits without one, Annals returns
`model_did_not_submit_reconciliation`.

No SQLite write transaction is held while the model examines the work. For an
ordinary unstructured work, the liaison can start at the beginning and pass a
read's exact `continue_after` quotation into the next read. Each result reports
`region_complete` and `work_complete`. A highly repetitive work may not yield a
unique continuation quotation; exhaustive sequential traversal is not
guaranteed in that case, so the liaison must use another available natural
anchor when possible.

## Language-level reconciliation

The host supplies the immutable work and base revision. The model submits one
strict object containing a summary, one or more operations, and optional
free-form annotations. It does not submit an outcome or confidence judgment.
Annotations preserve meta-level observations about this reconciliation, have
no validation, review, confidence, or application semantics, and do not
replace source-derived content in grounded operations.

Existing concepts use complete path arrays from the frozen base revision:

```json
{"path":["Database systems","Concurrency control"]}
```

Concepts created in the same reconciliation use their meaningful label:

```json
{"new":"Predicate locking"}
```

Created labels must be unique within the request after Unicode NFKC,
lowercasing, and whitespace collapse. Existing paths and
reconciliation-local labels are all resolved before operations execute. A move
or reword therefore cannot silently change what a later selector denotes.

Evidence uses an exact quotation from the scoped work. When it is repeated,
`within_heading` (an exact heading path), `preceded_by`, or `followed_by`
disambiguates it. The public protocol never asks for a byte offset.

Placement appends by default. Optional `before` and `after` selectors express
relative ordering; they are mutually exclusive. Omitting `under` places a
created or moved concept at the root level.

## Resolution and validation

Submission parses a strict JSON contract and projects the complete resulting
corpus in memory. An already-satisfied evidence mapping is idempotent: it
continues to express the requested relationship without creating a duplicate
row. Annals validates, among other things:

- every path and reconciliation-local selector resolves;
- quotations resolve uniquely in the immutable work;
- roots and siblings have normalized-unique labels;
- ordering anchors belong to the selected destination;
- the result is one acyclic ordered forest;
- every leaf has source evidence;
- evidence ranges are unique valid UTF-8 ranges;
- retirement leaves no children behind; and
- rewording explicitly retains or removes existing evidence.

Annals does not repair a reconciliation, assign it a confidence level, or
judge whether its conceptual claims are true. It validates the deterministic
boundary around that judgment.

A stored reconciliation includes its original request, resolved semantic
operations, and complete projected result. If that projection differs
mechanically from the base, the current result is `pending`. A result based on
the same or a later revision supersedes the same work's pending
reconciliation. An older-base result remains an examination record without
displacing a newer pending reconciliation.

If the projected corpus is mechanically equal to the base, Annals stores the
reconciliation with status `recorded`. This equality describes only the
materialized corpus state produced by this interpretation; it makes no claim
about semantic completeness or future usefulness. Annotations are excluded
from the comparison.

## Atomic application

Application requires `HEAD == base_revision`, re-resolves the stored request,
and verifies that it produces the same transition. One immediate SQLite
transaction then:

1. materializes the resulting concepts and evidence;
2. rebuilds derived concept-search rows;
3. inserts the append-only commit with request, resolved operations, metadata,
   and before/after snapshots;
4. marks the reconciliation applied;
5. advances the revision once; and
6. commits all state together.

Any error rolls back the entire transition. A stale reconciliation fails with
`stale_change`; Annals does not automatically rebase it. Annotations never
block application; choosing not to apply a pending reconciliation is an
operator decision.

## History and reversion

HEAD is materialized for ordinary reads. Every commit stores sufficient
before-and-after state for historical `show`, semantic `diff`, validation, and
inversion.

`revert REVISION` applies the inverse of that revision to current HEAD and
creates a new `revert` commit. It never removes the original commit. Inversion
checks that each affected field still matches the target revision's after-state;
otherwise it fails atomically with `revert_conflict`.

Retired concepts disappear from HEAD but remain represented in historical
snapshots. Works are independently retained and are never deleted by concept
retirement, reorganization, or reversion.
