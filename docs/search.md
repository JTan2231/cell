# Search design

Annals starts with local, embedding-free search. SQLite is the source of truth and
FTS5 provides lexical retrieval. The tree is not the search index: search first
finds matching topics and source passages, then uses the tree to scope, group, and
present those matches.

This design aims for predictable behavior, useful diagnostics, and a small amount
of machinery. It leaves room to tune ranking without making ranking part of the
stored data model.

## Goals

- Find exact topic titles, paths, identifiers, phrases, and uncommon terms reliably.
- Search topic and source bodies at an appropriate passage size.
- Restrict a query to a subtree or a kind of node.
- Avoid filling the result list with adjacent levels of the same branch.
- Explain each result in terms a CLI user can understand.
- Keep all indexes rebuildable from canonical node titles and bodies.
- Produce stable ordering for the same database and query.

Not included here are semantic search, generated query expansions, generated
summaries, or generated topics.

## Searchable units

The unit indexed by FTS5 is not necessarily a node.

- A short topic or source body normally contributes one unit containing its
  title and body.
- A long topic or source body contributes ordered passage units, all pointing
  back to the same node.

Passage boundaries are an indexing concern and do not add nodes to the user's
tree. Each unit needs, at minimum:

```text
id
node_id
unit_kind          node | passage
unit_no            passage order within the node
title              owning node's title
normalized_title   normalized title used for exact lookup
breadcrumb         titles only, from the search root to this node
normalized_path    normalized breadcrumb/path used for exact lookup
text               this node's body or this passage's slice of that body
start_byte         optional byte offset in the canonical node body
end_byte           optional byte offset in the canonical node body
content_hash
```

Use stable node IDs, but treat unit IDs as index implementation details. Reindexing
may replace units. `content_hash` makes unchanged units easy to recognize and helps
diagnose stale indexes.

Chunk a long body on natural boundaries where possible: headings, paragraphs,
then sentences. A starting target of roughly 800 to 1,500 words with a small
overlap is adequate; the exact values should be set by evaluation rather than
made part of the public format. Do not create many tiny chunks, and do not allow
one very long node to gain rank merely by producing many chunks.

## Indexes

Keep an ordinary B-tree index for normalized exact lookup and an FTS5 table for
lexical lookup. Logically, the indexed fields are:

```sql
CREATE INDEX search_units_by_normalized_title
    ON search_units(normalized_title, node_id);

CREATE INDEX search_units_by_normalized_path
    ON search_units(normalized_path, node_id);

CREATE VIRTUAL TABLE search_fts USING fts5(
    title,
    breadcrumb,
    text,
    content = 'search_units',
    content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);
```

The final schema may name these objects differently. If an external-content FTS
table is used, mutations must update `search_units` and its FTS rows in the same
transaction. A `reindex` command must be able to discard and rebuild all search
units and FTS data from canonical titles and bodies.

Index the title separately from the body so it can receive more BM25 weight.
Breadcrumbs are useful for queries such as `sqlite indexing`, but should receive
less weight than the node's own title: matching an ancestor's title is context, not
a direct match on every descendant. Do not copy ancestor bodies into descendants.

The `unicode61` tokenizer is a reasonable default for prose. Its behavior for
hyphens, underscores, dotted identifiers, and non-Latin text needs fixture tests.
If code-like sources become important, add a deliberately chosen tokenizer or a
separate identifier field after measuring failures; do not begin with a custom
tokenizer.

## Normalized exact lookup

Before FTS, resolve exact references and compare normalized titles and paths. Do
normalization in Rust so reads, writes, imports, and tests use the same rules:

1. Trim leading and trailing Unicode whitespace.
2. Apply Unicode NFKC normalization.
3. Apply Unicode lowercase expansion consistently.
4. Collapse each internal run of Unicode whitespace to one ASCII space.

Do not remove punctuation. `C`, `C++`, `C#`, and `Clojure` must not normalize to
the same key. Keep the original text for display.

Exact lookup should recognize, in order:

1. An unambiguous stable node identifier or explicit path reference.
2. A normalized full path.
3. A normalized title within `--within`, if unambiguous.
4. A normalized title anywhere in the allowed scope.

Duplicate titles are valid. Return all applicable matches with breadcrumbs rather
than silently choosing one. An exact match is a strong ranking signal, not normally
a reason to suppress other results; users often type a broad topic title intending
to see relevant sources beneath it.

## Query input and escaping

The default query language should be intentionally small:

- Whitespace separates terms.
- Double quotes preserve a phrase.
- CLI flags express filters; do not overload the query string with a second filter
  language initially.
- An unmatched quote is treated as literal input or reported as a concise parse
  error, consistently across commands.

Never pass raw input directly to the right side of `MATCH`. Binding the `MATCH`
string as a SQL parameter prevents SQL injection, but it does not prevent user text
from being interpreted as FTS5 syntax. A query builder must quote FTS tokens and
phrases, double embedded quote characters according to FTS5 rules, and append only
operators created by Annals itself.

Use an AND query for the first lexical pass. If it produces too few candidates,
run a controlled OR pass and mark those matches as lower-confidence fallback
results. Quoted phrases remain phrases in both passes. Ignore empty tokens and
place modest limits on query bytes and token count so accidental pasted documents
do not turn into pathological queries.

Punctuation-only and whitespace-only queries should not execute an unrestricted
FTS scan. They return either an exact identifier/path result or a clear `empty
search query` error.

## Candidate collection

Search has a small, fixed set of retrieval passes:

1. Collect exact identifier, path, and normalized-title matches.
2. Collect the top lexical matches from FTS5 using an AND query.
3. If the candidate set is sparse, collect lower-confidence OR matches.
4. Optionally, if it is still sparse, run a prefix or typo fallback over titles.

Fetch more candidates than the requested result count so grouping and branch
diversity have room to work. A practical initial value is 50 to 100 FTS units for a
default result limit of 10. Put these values in one search configuration structure,
not throughout the code.

Use FTS5's `bm25()` to rank lexical matches. Give `title` the largest column
weight, `breadcrumb` a modest contextual weight, and `text` (the indexed body
passage) the baseline weight. The exact values are tuning parameters. SQLite
returns better BM25 matches as more negative/lower values, so wrap that detail
in the search repository rather than letting callers depend on its sign or
magnitude.

Do not compare raw BM25 values to hand-authored bonuses as if the scale were
stable. Convert the ordered lexical list into a bounded rank contribution, for
example:

```text
lexical_score(rank) = 1 / (K + rank)
```

where rank starts at one and `K` is a small constant. Exact-match classes can then
be represented as explicit priority fields instead of fragile large numeric
bonuses.

### Prefix and typo fallback

Prefix and typo matching are fallbacks, not part of every query.

- Permit prefix lookup for the final unquoted token only, with a minimum token
  length (for example, three characters).
- Apply it primarily to titles, not every source body.
- If typo tolerance is needed, use a separate FTS5 trigram index over normalized
  titles and paths, or edit distance over the already narrowed topic vocabulary.
- Require a stricter threshold for short strings; one-character titles and common
  two-letter abbreviations should not produce a large fuzzy result set.

Do not build a trigram index over all source material until corpus measurements
show that its space and noise are worthwhile. A typo result must be identified as
such in diagnostics and ranked below exact and ordinary lexical matches.

## Filters and scope

The initial CLI surface can remain small:

```text
annals search QUERY
    [--within NODE]
    [--kind topic|source|all]
    [--detail overview|balanced|source]
    [--limit N]
```

`--within` is a hard filter. Resolve its node reference first, then use a recursive
CTE over the adjacency-list tree to select that node and its descendants. Join the
resulting node IDs to FTS units. If FTS performance within a very large subtree is
poor, measure before adding a closure table or materialized path.

`--kind` is also a hard filter. `--detail` is a presentation/ranking preference:

- `overview` prefers topic nodes when direct relevance is comparable.
- `balanced` has no blanket depth preference.
- `source` prefers source leaves when direct relevance is comparable.

Detail preference must not outrank an exact title/path match or a substantially
better lexical match. Search the full permitted scope first; do not traverse from
the root by selecting one apparently relevant branch, because an early wrong
choice would hide valid descendants.

## Rolling passage matches up to nodes

FTS returns units, while the public result is a node. Group units by `node_id`
before tree reranking.

Use the best passage as the node's direct lexical score. A second independently
matching passage may add a small, capped bonus, but do not sum every matching
passage. Summation systematically favors long nodes and repeated text.

An implementable starting rule is:

```text
node_direct = best_unit_score + min(0.15 * second_unit_score, cap)
```

Keep at most two passages for display. Prefer non-overlapping passages; overlapping
chunks that matched the same terms count as one piece of evidence. Exact matches
on a node's title or path are attached to the rolled-up node, not duplicated once
per passage.

## Tree-aware support

A direct match means the node's own indexed fields matched. A supporting match
means a descendant matched. Keep these concepts separate in both code and output.

Propagate only the strongest descendant signal upward, decayed by tree distance:

```text
support(node) = max(
    descendant_direct * decay ^ distance(node, descendant)
)
```

A decay around `0.6` to `0.8` is a reasonable test range. Never sum support from
all descendants: large, broad branches would otherwise outrank focused branches
merely because they contain more material.

A simple final score can be:

```text
final_score = node_direct + support_weight * support
```

with support weighted below direct relevance. Supporting-only ancestors may be
useful as group headings or overview results, but they must not displace their
stronger direct descendant by default. Exact-match class remains a separate
top-level ordering signal.

Ancestor chains can be loaded for the bounded candidate set with a recursive CTE.
There is no need to propagate scores across the entire tree on every query.

## Collapsing ancestor and descendant results

After node roll-up and support propagation, group candidates that lie on the same
ancestor/descendant chain. This prevents results such as `Databases`, `SQLite`,
`SQLite indexes`, and one source below it from occupying four independent slots
for the same evidence.

Choose one primary result per chain using:

1. Exact direct match over non-exact or supporting-only matches.
2. Higher direct relevance over inherited support.
3. The requested `--detail` preference when relevance is close.
4. Higher final score and then the deterministic tie-breakers below.

Attach other direct matches in that chain as related hits, including their own
snippets. Do not collapse cousins: two descendants in different child branches
are independent results even though they share an ancestor. If later usage shows a
need to expose every level, an `--all-levels` diagnostic option can bypass this
step, but it is not required for the first release.

## Branch diversity

One branch can still dominate after chain collapsing. Apply a simple greedy quota
using the first child below the search scope as the branch key:

1. Walk the ranked list and accept at most two primary results per branch.
2. Keep skipped results in order.
3. If the result limit is not filled, backfill from the skipped list.

The scope node itself has its own branch key. Exact path, identifier, and title
matches are exempt from the initial quota, though their related descendants still
collapse normally. This policy is deliberately simpler than a general diversity
optimizer and is easy to explain and test.

## Snippets and result contract

Generate snippets from the actual matched unit, not from concatenated subtree
text. FTS5 `snippet()` or `highlight()` can select lexical context; use internal
sentinel markers and let the CLI renderer convert them to ANSI styling only when
color is enabled. Strip or visibly escape control characters from indexed body
passages before terminal display.

For passage units, retain canonical body offsets so a result can identify the
containing range even if FTS5 does not provide exact match offsets.
For a topic-title match with no useful body match, show the title and breadcrumb
without inventing a body excerpt.

The search layer should return structured results, independent of terminal
formatting:

```text
SearchResult {
    node_id
    kind
    title
    breadcrumb
    primary_unit_id?
    body_range?
    snippet?
    match_reasons[]       exact_id | exact_path | exact_title |
                          phrase | lexical | prefix | typo |
                          descendant_support
    direct_score
    support_score
    related_hits[]
}
```

Scores are diagnostic, local to the current index and ranking version, and are not
promised to be comparable across releases. Human-readable output should emphasize
breadcrumb, kind, and match reason. A `--json` representation, if added, should
carry a result schema version.

## Deterministic ordering

Once candidates have their rank fields, sort with an explicit, stable sequence:

1. Exact-match class: identifier/path, then full title, then none.
2. Final score descending.
3. Direct score descending.
4. Raw BM25 value ascending for lexical ties.
5. Normalized breadcrumb ascending.
6. Stable node ID ascending.

Unit-level ties additionally use passage ordinal and unit ID. Do not use update
time as an implicit tie-breaker; recency is not relevance unless a future flag
asks for it. Offset pagination is deterministic while the database is unchanged,
but mutations may shift results, which is acceptable for a local CLI.

## End-to-end algorithm

```text
parse query and flags
resolve --within and any explicit node reference
normalize query for exact lookup

exact_units = exact lookup under hard filters
fts_units   = top lexical AND matches under hard filters

if sparse:
    add lexical OR matches
if still sparse and eligible:
    add prefix/typo title matches

deduplicate units
roll units up to owning nodes
load candidate ancestor chains
compute strongest-descendant support
collapse ancestor/descendant chains
sort with explicit tie-breakers
apply branch quota, then backfill
produce snippets and structured results
```

Every fallback decision and ranking stage should be visible in a debug mode. A
useful diagnostic line identifies the unit, raw BM25 value, lexical rank, exact
class, direct score, support source, chain group, branch key, and final position.
This is more valuable during tuning than a complicated ranking model.

## Evaluation and tuning

Build a small checked-in relevance set before adjusting weights. It should contain
queries for:

- Exact and duplicate topic titles.
- Full and scoped paths.
- Quoted phrases.
- Rare words, acronyms, punctuation-heavy titles, and code identifiers.
- Multi-term queries with one absent term, exercising the OR fallback.
- Misspellings and incomplete final terms.
- Broad topics whose best presentation is an internal node.
- Precise questions whose best hit is a source passage.
- Queries with several relevant branches.
- Queries where adjacent ancestor levels all contain similar language.

For each query, record acceptable nodes and, when important, preferred result
groups. Track at least:

- Recall at 5 and 10.
- Reciprocal rank of the first acceptable result.
- Number of redundant same-chain primary results.
- Number of represented branches in the first 10.
- Query latency at representative corpus sizes.

Golden tests should also assert parsing, escaping, scope filters, deterministic
ties, snippets, and match reasons. Tune FTS column weights, candidate counts,
support decay, and branch quota one at a time against this set. Avoid optimizing
only for a handful of memorable examples.

## Edge cases and invariants

- Empty, whitespace-only, and punctuation-only input never becomes a full scan.
- Duplicate titles remain distinct and always include breadcrumbs in output.
- Moving or renaming a node invalidates indexed breadcrumbs for that subtree.
- Editing a long body replaces its passage units and FTS rows transactionally.
- Deleted nodes leave no FTS rows; `reindex` detects and repairs drift.
- Cycles and dangling parents are rejected by tree mutation code before search.
- Overlapping chunks do not count as independent ranking evidence.
- Repeated boilerplate does not accumulate support up the tree.
- A scope with no searchable body returns an empty result, not global matches.
- Non-text and undecodable source material is skipped with an index status the CLI
  can report; it is not silently indexed as damaged text.
- Very common terms may produce many equal-quality matches, so stable IDs and
  breadcrumbs must make the result order repeatable.
- FTS syntax characters, quotes, terminal escapes, and control characters are
  handled as data unless Annals itself deliberately creates an operator.
- A failed optional prefix or typo pass never removes results from the ordinary
  lexical pass.

## Initial implementation boundary

The first complete version needs only normalized exact lookup, FTS5 BM25, subtree
and kind filters, node roll-up, strongest-descendant support, chain collapsing,
branch quotas, snippets, and deterministic ordering. Prefix search can follow
quickly. Trigram typo handling should be added only when evaluation demonstrates
that misspellings are a meaningful failure mode.

This boundary preserves a straightforward execution path while establishing the
interfaces and tests needed to tune search behavior later.
