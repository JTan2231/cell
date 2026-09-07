# Experiment 3: Twenty-chat medium versus high

## Result in one sentence

Medium built a flat glossary in less model time. High consolidated, nested, enriched, and retired concepts, and populated `uncertainties` on nine proposals. The experiment's application policy left those nine proposals pending.

## Controlled setup

- 20 identical rendered works in the same order: 418,410 UTF-8 bytes, 69 user messages, and 232 assistant messages.
- Both arms began as copies of one validated revision-zero seed database.
- Medium used `gpt-5.6-terra` with medium reasoning; high used `gpt-5.6-sol` with max reasoning.
- Both used `liaison-v1` and the same snapshotted Annals binary.
- Execution order alternated by work so each arm ran first ten times.
- A change was applied only when its `uncertainties` array was empty. There were no human edits.

This compares two sequential corpus trajectories across 20 works.
The models chose different concepts from the start. Their revision trajectories
first diverged at work 3, when high populated `uncertainties` and the runner
left its proposal pending; subsequent runs saw their arm's accumulated corpus.

## Cohort

The cohort retained the three conversations used in experiments 1 and 2, two
safe agent-interface conversations, and fifteen Annals conception, design, and
implementation conversations. It is intentionally coherent but strongly
project-weighted; it is not a random sample of 20 conversations from the
original self-analysis directory.

Two otherwise eligible sessions were excluded because they contained login,
one-time-code, private-repository, or token-related material. The renderer kept
only human-visible user and assistant messages. Visible messages from rolled
back turns remained present to match the earlier rendering method. Works 16–18
also contain copied-forward context from earlier conversations.

The exact source selection is in `manifest.json`; `manifest.lock.json` records
the completed run's rendered hashes, source hashes, byte counts, and message
counts.

## Aggregate outcome

| Metric | Medium | High |
| --- | ---: | ---: |
| Applied / pending proposals | 19 / 1 | 11 / 9 |
| Final concepts / roots / maximum depth | 21 / 21 / 1 | 13 / 8 / 3 |
| Final evidence links | 27 | 33 |
| Proposed operations | 22 | 28 |
| Proposed evidence selectors | 29 | 59 |
| Model time | 548.442 s | 2,757.305 s |
| Successful / attempted tool calls | 134 / 143 | 266 / 285 |
| Successfully executed query strings | 213 | 1,237 |

High used 5.03 times the model time, 1.99 times the tool calls, 5.81 times the successful query strings, and 2.09 times the read regions.

## What medium built

Medium's 21 final concepts are all roots. Every one of its 22 proposed operations was `create_concept`; it never placed a concept beneath another concept, added evidence to an existing concept, or retired superseded material.

The complete live corpus was:

- `Conversation family graph`
- `AI as an external judgment loop`
- `Frame construction environment`
- `Agent-compatible web surfaces`
- `Flat retrieval with tree-aware result presentation`
- `Strict local CI gate`
- `Annals CLI version-one implementation`
- `Producer-specified final topology and content`
- `Durable node IDs versus disposable search-unit IDs`
- `GitHub-only release version chain`
- `Model judgment separated from deterministic validation`
- `Raw ingested text is retained for grounding but is not searched`
- `Search indexes generated labels and breadcrumbs`
- `Retrieval broadens from AND to OR and prefix matching`
- `Whole-file single-prompt segmentation`
- `Corpus reconciliation`
- `Declarative change requests`
- `Semantic proposal boundary between model and Annals mechanics`
- `Single write boundary for model liaisons`
- `Ordered concept forest`
- `Ordered multi-parent concept graph`

Each concept links to an exact source quotation. Their relationships remain implicit in this run, whose output forms an evidence-linked glossary.

## What high built

High's accepted corpus has eight roots and a depth-three hierarchy. Its strongest cluster is:

```text
Liaison-mediated corpus reconciliation
├── Semantic corpus-change interface
│   └── Pointer-scoped liaison sessions
├── Corpus-owned concepts and immutable works
└── Append-only reversible corpus history
```

The complete live corpus was:

```text
Fork-aware conversation classification

Longitudinal conversation-archive review

Strict local CI gate

Declarative tree patch contracts

Canonical text forest with rebuildable search projection
└── Label-and-breadcrumb retrieval with staged lexical broadening

GitHub-only release and versioning contract

Structural resolution controls for model-generated trees

Liaison-mediated corpus reconciliation
├── Semantic corpus-change interface
│   └── Pointer-scoped liaison sessions
├── Corpus-owned concepts and immutable works
└── Append-only reversible corpus history
```

Across all proposals high used 25 creates, two retirements, and one evidence
addition.

Work 18 is the clearest contrast. Medium created `Single write boundary for model liaisons` as one new root with one quotation. High made five coordinated operations with ten quotations: it enriched the existing semantic interface, created three placed refinements, and retired an obsolete whole-file-ingestion concept introduced four works earlier. That is genuine corpus reconciliation rather than per-document extraction.

## Why high's final corpus is smaller

High proposed more operations and twice as many evidence selectors, and populated `uncertainties` on nine works. Under the experiment's application policy, all nine remained pending.

The entries described Computer Use guidance and several source or corpus conditions:

- no existing parent path was found;
- the source described a recommended design;
- the source stated a qualification on its interpretation.

These entries are retained in the proposal records. Under that application policy, their presence determined which proposed operations entered the corpus seen by subsequent runs.

Seven of high's nine pending proposals are now stale relative to HEAD. The last two share base revision 11, so applying either would stale the other. Medium's one pending proposal is also stale. They must be re-examined or resubmitted rather than applied as a batch.

The nine retained high proposals that did not enter the live tree were:

1. `AI-assisted frame construction`—the archive as a setting for making latent
   structure actionable, qualified by feedback-loop convergence.
2. `Canonical content projection for human and LLM front ends`—one canonical
   content model projected into human and machine-facing surfaces.
3. `Background-capable but non-isolated Computer Use on macOS` and
   `Single-controller guidance for multi-agent Computer Use on one Mac`.
4. `Embedding-free topic-tree retrieval`—flat lexical candidate search with
   tree-aware scoping, context, grouping, and ranking.
5. `Annals v1 CLI implementation`—the reported completed capability set.
6. `Canonical node text as the searchable corpus`, proposed beneath the
   canonical text forest.
7. `Semantic change requests resolved into exact corpus transitions`, together
   with retirement of the older declarative tree-patch contract.
8. `Progressive scope refinement without fixed level semantics`, beneath
   structural resolution controls, and `Cross-cutting concepts in
   single-parent trees`, beneath the canonical text forest.
9. `Ordered multi-parent DAG with a designated primary placement`, beneath the
   canonical text forest.

Medium's sole pending proposal concerned single-controller Computer Use
guidance. Pending records retained their operations, exact quotations,
summaries, `uncertainties` entries, and frozen base revisions.

## What each database retained

Each arm contained all 20 immutable rendered works, 20 model-run records, 20
model proposals, the liaison tool-call transcript for each examination, and a
revision history for every accepted transition. The live concept tree was only
the current projection of the applied proposals. Thus a concept absent from
the tree could still be present—and fully evidence-grounded—in the proposal
history.

## Agreement and provenance

The final corpora share only one exact path, `Strict local CI gate`. They share four exact source spans out of 27 medium and 33 high spans (Jaccard 0.0714). Across all proposals, including pending ones, they share 12 source spans out of 29 and 59 (Jaccard 0.1579). Source-span selection overlapped more often than naming, placement, and application outcomes.

Final evidence is overwhelmingly assistant-authored: medium has 26 assistant spans and one user span; high has 31 assistant spans, one assistant-commentary span, and one user span. Each link identifies a selected quotation from the retained conversation. Works 16–18 include copied-forward context from earlier conversations.

## Practical conclusion

Under this experiment's unattended policy, medium applied 95% of works at one-fifth the model time. High used more consolidation and historical-correction operations, while the runner left its proposals with `uncertainties` entries pending.

A subsequent comparison could hold the corpus revision fixed for each work and replicate trials. That would allow the reported operation choices and runtime to be compared independently of the accumulated corpus trajectories and application policy.

Both databases validate cleanly, their SQLite integrity and foreign keys are sound, and all locked input hashes match.

Before removal, the seed, medium, and high databases had SHA-256 values
`7dc1a1a65924d27b965ad2d7211e34ddbc25c6902ae5985601d8e29e86a26b59`,
`1b2edcdceb1c3f2cc2bb632e6dcdbc1ca2cdbfaa4772317b83cb33f606de1b15`,
and `616c89786857dfcd9c728fb21a9d7d5e6f153a42564668f512febcdbfcff98bc`
respectively. The database files, private transcript snapshots, model logs, and
generated raw reports are intentionally not retained.
