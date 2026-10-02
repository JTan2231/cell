# Read and search an Annals corpus

Use this feature to inspect concepts, relationships, exact supporting quotations,
and immutable corpus history. Reads are local, bounded, and invoke no model or
network. `annals.libraries` owns library selection, common output, and read
prerequisites. `annals.work.retain` owns retained text and delivery-activity
meaning. Use `annals.corpus.change` for reconciliation and history mutations.

## Corpus meaning and authority

The library owns one corpus of concepts, explicit parent edges, and evidence
links. Its stored instructions define the interpretation of concepts and edges.
The default frame uses broader/narrower scope. Historical commits can have
earlier instruction selections; current instructions do not reinterpret them.

Concepts have durable lowercase `cN` IDs, with a positive canonical decimal
integer. A label describes a concept and can repeat; it never selects one.
Rewording and relationship changes preserve identity. Concepts have no canonical
path, primary parent, sibling position, or move semantics.

Parent edges are explicit, untyped, unordered relationships in a directed
acyclic graph. Parent and child describe one direct edge; ancestor and descendant
describe one or more edges of reachability. A direct edge can coexist with a
longer route. A root has no parents, a leaf has no children, and a shared concept
has several parents. These properties are derived from the selected edge set.

Evidence links associate one concept with one exact quotation occurrence in
an immutable retained work. Evidence supports the concept across its parent
relationships and does not belong to an edge. Every leaf requires evidence;
other concepts can also have it. This invariant does not establish a concept's
truth or require direct evidence on every concept. Source-heading paths locate
document text, not concepts. Public read output exposes quotations, not internal
byte ranges. Each retained evidence range has an 8 KiB cap.

## Select and inspect a revision

HEAD is the default. Commands with `--at REVISION` select an immutable historical
corpus state. The supported corpus interfaces are:

```text
annals overview [--at REVISION]
annals roots [--at REVISION] [--limit N] [--cursor TOKEN]
annals concept show cN [--at REVISION] [--preview-limit N]
annals concept parents cN [--at REVISION] [--limit N] [--cursor TOKEN]
annals concept children cN [--at REVISION] [--limit N] [--cursor TOKEN]
annals concept evidence cN [--at REVISION] [--limit N] [--cursor TOKEN]
annals graph cN [--at REVISION] [--direction parents|children|both]
  [--depth N] [--max-nodes N]
annals search QUERY [--at REVISION] [--within cN]
  [--limit N] [--cursor TOKEN]
```

`overview` reports revision-wide counts of concepts, explicit edges, roots,
leaves, shared concepts, and evidence. It does not dump the graph. `roots`
pages through summaries with no parents. `concept show` returns ID, label,
relationship/evidence counts, root/leaf/shared flags, and bounded previews.
`parents` and `children` page through compact `{id, label}` references.
`evidence` pages through retained-work and exact-quotation pairs.

`graph` expands a local neighborhood. Direction selects incoming parents,
outgoing children, or both; the default is `children`. Each concept appears
once even when several routes reach it. Depth measures explicit edge hops,
defaults to 2, and is 0–5. Maximum nodes defaults to 100 with a ceiling of
500. The response names its seed by ID, stores each selected label once in
`nodes`, and uses `{parent_id, child_id}` references for edges. A frontier marks
where depth or node limits stopped expansion. It does not identify corpus leaves
or establish that the full graph was read.

## Lexical search

Search matches concept labels and derived ancestor-label context. It does not
search source text, use semantic similarity, or perform web research. A narrowly
named concept can match labels of any ancestor. Ancestor context is a retrieval
aid, not another edge or identity. Inspect parent reads or graph output to
understand the actual relationships behind a match.

Normalization applies Unicode NFKC, Unicode lowercase expansion, then collapses
Unicode whitespace to one ASCII space. Punctuation remains. The normalized
query splits on whitespace and must be nonempty. The input ceiling is 512 UTF-8
bytes and 16 normalized terms; the result limit must be positive.

Every query term must occur in the normalized label or ancestor context.
Duplicate ancestor labels are ignored. Exact label matches rank first, then
label prefix matches, then broader label-term coverage. Public ID is the final
deterministic tie breaker. `--within cN` limits candidates to concepts below
that concept at the selected revision. A shared concept remains one result;
equal labels with different IDs remain different results. No preferred path is
constructed or returned.

JSON returns `revision`, `query`, optional `within` with ID and label,
and `results` with `items` and `page`. Each item contains `concept` with `id`,
`label`, `parent_count`, `child_count`, `evidence_count`, `root`, `leaf`, and
`shared`.
If nothing matches, search succeeds with empty `items`. Human output shows ID,
label, and compact graph/evidence counts. The liaison's revision-scoped
`corpus_search` tool uses this same lexical lookup with separate query cursors.

## Pages and completeness

Paged corpus responses contain `items` and `page` with requested `limit`,
`returned`, `total`, and optional `next_cursor`. A complete page sequence has no
further cursor. Later pages may request another limit. Cursors are opaque and
bind the same library identity, command, query, scope, and resolved revision.
Restart pagination when that context changes. Deterministic display order has
no conceptual ordering meaning.

Root pages default to 20 items. Concept inspection previews default to 5 items
for each direct neighborhood. The liaison's separate tool page limits belong
to `annals.work.integrate`. A bounded page or graph response is complete only
within its reported selection and limits.
It is not a promise of bounded total replay cost.

## Sources, activity, and history reads

These supported reads complement corpus exploration:

```text
annals work list [--limit N]
annals work show LABEL
annals lately [--since TIME] [--until TIME] [--by BASIS]
              [--status STATUS] [--channel CHANNEL]
annals log [--limit N]
annals diff FROM_REVISION TO_REVISION
annals change show --at REVISION
```

`work show` returns the complete unchanged source. `lately` reports deliveries,
not document topics or internal dates. Its timestamp basis controls window
membership and ordering; missing timestamps can omit deliveries. The retained
sources feature owns those fields, timestamp meanings, and recovery choices.

The commit log contains applied reconciliations, confirmed nonempty shakes,
and reverts. Retention, examinations, pending and no-change reconciliations,
and failures are not corpus transitions. `diff` reports exact concept, edge,
and evidence effects without inventing moves or ordering. The corpus-change
feature owns complete commit, diff, and reconciliation output meaning.

## Replay and Rust access

Current and historical corpus reads use the same canonical reducer. Revision
zero is empty; revision N replays contiguous commit effects from 1 through N.
Annals stores no separate current-graph snapshot or cache that needs
synchronization. Reducer failure invalidates the library; stop and follow
library recovery rather than skipping a revision or editing effects.

Bounded queries project the selected in-memory `CorpusState` into connection-local
temporary concept, edge, and evidence tables with indexes. The tables are
disposable query acceleration, not persistent state or authority, and stay in
memory. For lexical search, matching labels propagate each term to descendants
while duplicate `(term, concept)` states are removed. Only the requested page
is converted to owned responses. This supplies ancestor-context matching
without a preferred route or persisted transitive ancestor set.

Replay and invariant checks grow with corpus history, concepts, edges, and
evidence links. The whole selected state stays in memory. Evidence reads load
only selected quotation ranges. There is no promised maximum corpus size,
throughput, query latency, unlimited graph expansion, or cursor retention time.

`annals::api::LibraryReader` returns the provider-owned read views using these
queries and cursor rules. `CliClient` can issue typed read `Request` values to
an explicitly selected executable and returns matching `Response` values.
`annals.libraries` owns complete client selection, construction, and effect rules.

## Recovery, privacy, and compatibility

Use IDs, not labels or paths, for concepts. Missing state, unknown IDs or
revisions, invalid bounds, and mismatched cursor context fail explicitly.
Read commands leave HEAD and library state unchanged and perform no setup or
repair. Read access does not authorize mutation or disclosure.

Labels, source text, quotations, and history can be private. This feature
sends none to a model or network service. CLI usage recording follows
`annals.libraries` and cannot change the read result. Combined search, graph
links, cursors, and transactions across libraries are unsupported.

Feature contract version 2, provider release, library schema, and output schema
are independent. No cross-release cursor compatibility, output support window,
deprecation period, or wall-clock objective is promised. Exploration relies on
the selected local Annals library, not a model or external service.
