# Find and inspect Geste episodes

Use Geste before beginning a request when its structural problem may have been
handled before. Geste is a manual local casebook, so retrieval returns
precedent candidates rather than a complete history or a judgment that a
candidate applies.

Select the database explicitly with `--database`, or let Geste resolve it from
nonempty `GESTE_DATABASE` and then
`$HOME/Library/Application Support/Geste/geste.db`. The CLI never falls back to
the current directory. Run `geste doctor` against the same selection if the
database's readiness is unknown.

Start with concise structural terms rather than project-specific wording:

```sh
/Users/joey/.local/bin/geste search \
  'cross product contract provenance' --limit 10
```

Search applies NFKC normalization, lowercase conversion, and whitespace
collapse, then requires 1 through 16 unique terms. Every term must match at
least one field in the latest revision. Each term contributes only its best
field weight: exact normalized tag 8, shape 6, title 5,
situation/response/applicability 3, and outcome/lesson 2. Candidates sort by
descending score and then ascending numeric episode ID. Outcome matching
includes both its status and summary. There is no stemming,
synonym expansion, embedding, model call, or semantic-similarity claim.

Inspect every plausible candidate rather than reusing the score:

```sh
/Users/joey/.local/bin/geste episode show e12
/Users/joey/.local/bin/geste report e12
/Users/joey/.local/bin/geste graph e12
```

Use `--at N` on show, report, or graph to select a historical immutable
revision. Without it, the latest revision is selected at read time. Add global
`--json` when a typed compact JSON value is more useful than the human output.

The report is a deterministic read-time view of the complete selected
revision. It distinguishes Geste-authored interpretation from structured
verified or unverified settlements and calls out coverage gaps and locator-only
sources. The graph is another read-time view of only that revision: typed
episode, claim, source, and related-episode nodes plus structural, support, and
coverage-gap and episode-relation edges. Structured source attributes and the
manual-source boundary keep the exact anchor basis visible. It is not retained
truth and does not recursively traverse the corpus.

Before following a precedent, compare its situation, applicability, outcome,
gaps, source cutoff, and exact cited revisions with the new request. Resolve
mutable or locator-only anchors and current procedure through the owning
product's installed Chancery contract. A successful old response can be
inapplicable now, and no lesson becomes policy merely because Geste retained
it.

A no-match result proves only that no manually captured latest revision
satisfied every lexical term. Refine the shape terms, inspect the casebook's
manual coverage, or proceed without a precedent; do not report that the problem
has never been solved.

Geste reads only its private SQLite state. It does not contact source products,
a model, or the network. Output can still disclose private process prose,
source labels, stable machine references, and outcomes; redirected output is
caller-owned retained data.
