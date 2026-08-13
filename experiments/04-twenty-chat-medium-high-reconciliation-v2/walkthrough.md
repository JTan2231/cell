# Experiment 4: Twenty-chat medium versus high under reconciliation v2

## Result in one sentence

The new reconciliation contract let both presets apply all 20 works, but medium
built a readable 142-concept synthesis while high spent 4.76 times as long to
build a much more exhaustive 463-concept, depth-eight historical graph; neither
preset ever revised or removed existing corpus material.

## Question and controlled setup

This experiment reran the exact 20 rendered conversations from experiment 3
against commit `59bf8658e97e886c8881396ac37383c054c3ce17`, which introduced
the `liaison-v2` best-current-reconciliation prompt and contract.

- The locked inputs are byte-for-byte identical to experiment 3: 20 works,
  418,410 UTF-8 bytes, 69 user messages, and 232 assistant messages.
- Both arms began as copies of one validated revision-zero database containing
  all 20 immutable works.
- Medium used `gpt-5.6-terra` with medium reasoning. High used
  `gpt-5.6-sol` with max reasoning. There was no explicit model override.
- Works ran sequentially in manifest order. Medium ran first on odd-numbered
  works and high ran first on even-numbered works.
- Every examination recorded one reconciliation. Every corpus-changing
  projection was applied immediately with `integrate --apply`; an equal
  projection would have remained `recorded` without a revision.
- There were no human edits, operator retries, or application decisions.
  Models could correct rejected tool calls inside their existing liaison
  session.
- Each arm accumulated its own corpus, so this is a comparison of two
  path-dependent trajectories rather than 20 independent paired trials.

The runner snapshotted its binary, inputs, manifest, and own source, verified
their hashes, held an exclusive lock, and retained the complete model/tool
record in the private run artifacts. The complete sequential run took
3:10:08 wall time.

## Aggregate result

| Metric | Medium | High |
| --- | ---: | ---: |
| Applied / recorded-equal reconciliations | 20 / 0 | 20 / 0 |
| Final revision | 20 | 20 |
| Concepts | 142 | 463 |
| Roots | 6 | 5 |
| Maximum depth | 4 | 8 |
| Evidence links | 516 | 1,166 |
| Reconciliation operations | 216 | 677 |
| Create / add-evidence operations | 142 / 74 | 463 / 214 |
| Annotations / works with annotations | 4 / 4 | 18 / 14 |
| Integration wall seconds | 1,980.704 | 9,425.121 |
| Tool calls / failed calls | 195 / 26 | 414 / 33 |
| Successful tool calls | 169 | 381 |
| Successful query selectors | 284 | 1,456 |
| Successful work-read regions | 338 | 398 |
| Successful corpus-inspect paths | 139 | 584 |

High used 4.76 times the integration time, 2.12 times the tool calls, 3.13
times the operations, 3.26 times the concepts, and 2.26 times the evidence
links. Its tool-call failure rate was lower, 7.97% versus medium's 13.33%, so
the size difference is not explained by failed execution.

The arms diverged immediately. After work 1, medium had created 14 concepts
with 42 evidence links; high had created 63 concepts with 106 links. All later
works then saw those different accumulated corpora.

## What medium built

Medium produced six relatively balanced roots:

| Root | Concepts in subtree | Evidence links | Maximum depth |
| --- | ---: | ---: | ---: |
| `Embedding-free topic-tree retrieval architecture` | 43 | 191 | 3 |
| `Conversation archive and indexing design` | 25 | 114 | 3 |
| `Computer Use interaction, isolation, and concurrency` | 20 | 23 | 3 |
| `Liaison-directed reconciliation of immutable works with a versioned conceptual corpus` | 19 | 109 | 3 |
| `Strict local CI as a design constraint` | 19 | 53 | 3 |
| `Web-browsing surfaces for agents` | 16 | 26 | 4 |

Its depth distribution was 6, 65, 70, and 1 concepts at depths one through
four. Medium used broad concepts with denser support: 3.63 evidence links per
concept on average. It attached later works to existing concepts 74 times and
created 142 concepts in total.

Medium is best understood as a compact conceptual synthesis. It preserved the
major domains and many qualifications while keeping the tree navigable.

## What high built

High produced five roots, but most of the corpus accumulated under one deep
implementation-history branch:

| Root | Concepts in subtree | Evidence links | Maximum depth |
| --- | ---: | ---: | ---: |
| `Embedding-free topic-tree retrieval` | 270 | 801 | 8 |
| `Conversation archive indexing` | 107 | 202 | 4 |
| `Web-browsing surfaces for agents` | 34 | 48 | 4 |
| `Proposed GitHub-only release and versioning contract for Annals` | 26 | 59 | 3 |
| `Strict local CI for the Annals Rust CLI` | 26 | 56 | 3 |

Its depth distribution was 5, 38, 206, 147, 37, 13, 16, and 1 concepts at
depths one through eight. The largest root alone contained 58.3% of all high
concepts. High averaged 2.52 evidence links per concept, reflecting a much
finer-grained decomposition rather than weaker total coverage.

High was substantially better at preserving successive proposals, reported
implementation states, limitations, and source stance. Its 18 annotations
explicitly distinguished such things as superseded segmentation proposals,
current versus proposed topology, earlier versus later release policies, and
time-bound implementation reports. Because annotations are inert under v2,
this nuance no longer blocked application as `uncertainties` did in experiment
3.

The cost is an unwieldy historical knowledge graph. High placed most liaison
and Annals implementation material beneath the retrieval root, producing paths
as long as 538 characters and a tree whose useful atomic facts are harder to
browse as a whole.

## Coverage and consolidation

Merging overlapping evidence byte ranges within each work, medium covered
107,760 of 418,410 source bytes (25.75%); high covered 237,264 bytes (56.71%).
High's extra size therefore represents materially broader source coverage,
not only duplicate linking.

Medium had 42 multi-work concepts out of 142 (29.6%). High had 129 out of 463
(27.9%). Medium's relative consolidation was at least as strong even though
high made more absolute evidence additions. Work 5 is the clearest high-quality
consolidation example: medium created a separate 20-node Computer Use root,
while high used nine creates and two evidence additions to place the material
under its existing web/Computer Use branch.

Other useful paired contrasts were:

- Work 1: medium made 14 broad concepts with 42 quotations; high made 63
  concepts with 106 quotations spanning requirements, export anomalies, fork
  ontology, ingestion, classification, storage, analysis, and delivery state.
- Work 3: both used exactly 39 evidence selectors, but medium created seven
  concepts while high created 20. High separated month-specific claims and
  epistemic cautions that medium grouped together.
- Work 14: medium performed nine evidence additions and created nothing. High
  performed 19 additions and created 11 narrow retrieval and segmentation
  concepts.
- Works 16 through 18: medium used 39 operations and 71 evidence selectors;
  high used 160 operations and 282 selectors to preserve successive drafts and
  alternatives. Those works contain copied-forward context, so this recurrence
  is interactional reuse, not independent corroboration.

## Pure accretion rather than editorial reconciliation

Both arms used only `create_concept` and `add_evidence`. Neither arm ever used
`move_concept`, `reword_concept`, `retire_concept`, `remove_evidence`, or an
equal no-op reconciliation. Every initially chosen label and placement
therefore survived through revision 20.

This is the most important weakness exposed by the run. The v2 prompt achieved
high autonomous throughput and much broader representation, but it did not
induce either preset to reorganize or prune a growing corpus. High's
exhaustiveness will compound unless a later work or dedicated consolidation
mode reliably exercises the corrective operations.

## Agreement

| Set comparison | Medium | High | Shared | Jaccard |
| --- | ---: | ---: | ---: | ---: |
| Exact final paths | 142 | 463 | 1 | 0.0017 |
| Unique work-plus-source spans | 510 | 1,105 | 228 | 0.1644 |
| Exact path-plus-work-plus-span links | 516 | 1,166 | 1 | 0.0006 |

The sole shared path was the root `Web-browsing surfaces for agents`. Only one
other exact label appeared in both corpora: `Two-phase local preparation and
GitHub publication`, at different paths. The one fully shared evidence link
was the shared web-browsing root citing the same source span from work 4.

The 228 shared source spans are more informative than the one shared complete
link: the models often agreed on useful evidence while choosing radically
different names, granularity, and placement. These exact lexical overlaps are
not a semantic-similarity score and discard multiplicity.

## Evidence provenance and renderer artifact

Final evidence roles were:

| Role | Medium | High |
| --- | ---: | ---: |
| Assistant answer | 465 | 940 |
| Assistant commentary | 30 | 144 |
| User | 21 | 73 |
| Synthetic renderer preamble | 0 | 9 |

Evidence establishes what the retained conversation said, not independent
truth. The high arm also interpreted the runner-injected sentence
`Recovered from a local Codex session. This transcript includes only
human-visible user and assistant messages.` as semantic content. Nine links
across nine works grounded three artifact concepts:

- `Conversation archive indexing › 2026 year-to-date archive-use review › Human-visible recovered-session boundary`;
- `Web-browsing surfaces for agents › Human-visible-only recovered Codex transcript`;
- `Embedding-free topic-tree retrieval › Recovered human-visible local Codex transcript`.

Medium did not model the preamble. Future transcript renderers should omit this
semantic-sounding wrapper or keep it outside the work's addressable content.

## Tool behavior

Medium made 30 submission attempts for 20 successful reconciliations; high made
28. Medium's ten rejected submissions comprised five ambiguous quotations,
four quotations not found exactly, and one invalid operation name. High's
eight comprised four ambiguous quotations, three missing quotations, and one
invalid operation name. Both models once used the obsolete `attach_evidence`
name and then corrected it to `add_evidence`.

Neither model used `within_heading`, `preceded_by`, or `followed_by` in any
evidence selector, including failed attempts. They resolved ambiguous quotes
by choosing longer unique strings. The main read-tool error was requesting 20
results where the tool limit was 10: 12 medium corpus-search failures and 17
high corpus-search failures, plus one high work-search failure.

These recoveries show that the strict tool boundary worked, but the repeated
limit and disambiguation errors suggest prompt or schema descriptions could be
more salient.

## Relation to experiment 3

Experiment 4 is not a prompt-only rerun of experiment 3. The inputs, models,
efforts, ordering, and independent trajectory design are the same, but three
things changed together:

1. the liaison prompt changed from `liaison-v1` to `liaison-v2`;
2. the proposal/outcome/uncertainty contract became the reconciliation and
   inert-annotation contract;
3. the application policy changed from withholding uncertain proposals to
   mechanically applying every changing projection.

The observed differences are nevertheless large. Experiment 3 ended with 21
medium and 13 high concepts, 27 and 33 evidence links, and nine unapplied high
proposals. Experiment 4 ended with 142 and 463 concepts, 516 and 1,166 links,
and 20 applied reconciliations in each arm. The old high arm performed two
retirements; neither new arm performed any corrective operation. These deltas
cannot be assigned to the pointer prompt alone.

## Practical conclusion

Medium is the better default corpus builder when latency, balance, and
browsability matter. It produced a coherent 142-concept map in about 33
minutes while covering one quarter of the source bytes.

High is preferable when the goal is near-exhaustive recall of atomic facts,
reported states, and qualifications. It covered more than half the source and
retained far more epistemic nuance, but required about 2 hours 37 minutes and
produced a 463-concept tree that needs consolidation.

A practical policy is medium by default, with high reserved for unusually
dense or high-value works. The next experiment should remove the renderer
preamble and explicitly test a consolidation pass—or a deliberately
superseding work—on a fixed mature corpus to determine whether move, reword,
retire, and evidence-removal operations can be elicited reliably.

## Validation, reproducibility, and retained artifacts

Both databases passed Annals validation with no issues, SQLite integrity
checks, foreign-key checks, commit-snapshot continuity checks, and final
snapshot-to-live-state comparison. Both indexes were current. Every work,
input snapshot, runner snapshot, and binary hash matched its lock/configuration.
All 40 process logs exited successfully.

The run used Codex CLI `0.146.0`. No random seed, temperature, or backend
sampling state was controlled, and there was one trial per work and arm. There
was no external judge or semantic correctness score.

The exact historical runner is preserved rather than retroactively hardened.
Its standalone `report` command does not independently require a complete
20-work run, and an interruption that leaves a model row marked `running`
requires explicit recovery instead of ordinary `resume`. Neither condition
occurred here; the completed databases and logs were independently checked for
20 successful submissions and 20 applied reconciliations per arm.

Durable artifacts retained in this directory are the source manifest, locked
manifest, exact runner, and this walkthrough. Private transcripts, databases,
logs, setup output, raw reports, and the snapshotted binary are not committed.

Important SHA-256 values from the completed run were:

- Annals binary: `661dfa81216b39398a40e6feb15881cd7fe9a6f9372ff712ee25d47e1e6044f9`;
- runner: `15e680199a52753c6c0489f8a93fbb41eb8d6a8943152f1b1e456a028e758c0c`;
- locked manifest: `2d54e423921d9f1d24e0398b5ee06bcd6bf88f19ea6b2564ab69ebb30f3d5b6b`;
- seed database: `d84d250ac2173b7c4ce575bc28c9e8993c6cc0e0d92848042c1763b058117faa`;
- medium database: `16288ddc59f0be118d3abe8a2255fb5b5b3ee223e7e23dcc180d34ce591c7ef3`;
- high database: `b597479b5064b73df67a0328da901f7f031184ef498b16ec0f5dcde41caadcff`;
- raw comparison JSON: `3f17ceb6092d6b76b24415ae415bfeb823586e12b332780720c8ef72cae44e6f`;
- raw comparison Markdown: `3012e25a9edd49d9c2894684be8437aeb2ae002aa1de8f667cd2028d766f9bac`.

The exact invocation was:

```sh
./ci.sh
python3 experiments/04-twenty-chat-medium-high-reconciliation-v2/run.py start \
  --manifest experiments/04-twenty-chat-medium-high-reconciliation-v2/manifest.json \
  --run-dir /tmp/annals-medium-high-20-v2 \
  --annals target/release/annals
```
