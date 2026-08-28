# Experiment 1: Three-chat medium baseline

## Question

Could Annals turn a small, conceptually coherent set of reflective
conversations into a useful evidence-linked corpus?

The answer was yes, but sparsely. The result was a compact thesis index with
strong provenance, one useful human refinement, and limited discovery through
lexical concept names.

## Inputs

The original `~/self-analysis` checkout no longer existed, but its top-level
Codex sessions survived under `~/.codex/sessions`. Three safe conversations
were selected to form a progression:

1. What a forked conversation is as an archival object.
2. What can responsibly be inferred from a year of conversations.
3. What conceptual function the 2026 conversations appeared to serve.

Only human-visible user and assistant messages were rendered into Markdown.
System prompts, hidden reasoning, tool traces, and subagent records were
excluded; human-visible assistant commentary was retained.

| Work | Visible messages | UTF-8 bytes | SHA-256 |
| --- | ---: | ---: | --- |
| Conversation ontology and archive design | 51 | 48,433 | `d9a471c2fbb34f19b9e1ad9e24afbe543b4f7c4a3938c23491f759253799194c` |
| Interpreting a year of conversations | 27 | 15,577 | `cdaf20199b1353bcd3491f9b4a321fcc0315bb97202a005f803e1f87e32448a4` |
| Frame construction in 2026 conversations | 10 | 15,028 | `62f540bb827aa87d62db3e818d7635cbd7c4dde751efdce83535f4098d2d9a5f` |
| **Total** | **88** | **79,038** | |

## Procedure

- Begin with a fresh revision-zero Annals library.
- Integrate the three works sequentially in the order above.
- Use `gpt-5.6-terra`, medium reasoning, and `liaison-v1`.
- Review each proposal before applying it.
- Preserve uncertain model proposals rather than silently rewriting them.

This configuration later became the `medium` quality preset.

| Work | Base revision | Model time | Tool calls | Result |
| --- | ---: | ---: | ---: | --- |
| Conversation ontology | 0 | 20.9 s | 6 | Applied unchanged as revision 1 |
| Year interpretation | 1 | 43.1 s | 7 | Applied unchanged as revision 2 |
| Frame construction | 2 | 21.0 s | 6 | Reviewed and replaced at revision 3 |
| **Total** | | **85.0 s** | **19** | |

Eighteen tool calls succeeded. One attempted to inspect an empty corpus path;
the liaison recovered and submitted normally.

## Human intervention

The first two model proposals were applied unchanged. For the third work, the
model proposed only a root named `Frame construction` and recorded two
uncertainties:

- no related parent was found;
- recurring vocabulary might reflect interactional convergence rather than
  independent correctness.

Review kept the root placement but promoted the substantive epistemic warning
into an evidenced child, `Feedback-loop risk`. The model proposal remains in
history as superseded; its human replacement produced revision 3.

The final history therefore contained three model runs, four proposals, and
three commits: two model-authored commits and one human-authored commit.

## Final corpus

```text
Fork-aware conversation views [2 evidence links]

Archive-use analysis [1]

Frame construction [1]
└── Feedback-loop risk [1]
```

| Metric | Value |
| --- | ---: |
| Revision | 3 |
| Works | 3 |
| Concepts | 4 |
| Roots | 3 |
| Maximum depth | 2 |
| Evidence links | 5 |
| Stored proposals | 4 |
| Pending proposals | 0 |
| Commits | 3 |

The revision history was entirely additive: no concepts were moved, renamed,
retired, or stripped of evidence.

## What the corpus said

The four concepts form a compact argument:

1. A forked conversation is one shared message family with separately
   readable and classifiable root-to-leaf views.
2. A conversation archive supports claims about how it was used, not an
   unrestricted biography of its author.
3. One important use was frame construction: conversation helped stabilize
   reusable ways of interpreting problems.
4. Reused assistant language can itself produce recurrence, so convergence of
   vocabulary is not independent proof that the vocabulary describes reality.

The evidence excerpts preserved the essential claims: messages shared across
forks should be stored once; each leaf view should be classified; archive
review should remain a usage map rather than biography; the archive became a
frame-construction environment; and assistant-summary reuse creates a
feedback-loop risk.

All five selected passages were assistant-authored. They establish what these
conversations claimed, not that the claims were independently verified against
the underlying archive.

## What it revealed about Annals

Annals behaved more like a sparse, evidence-linked thesis index than an
exhaustive summarizer: 79,038 bytes of dialogue became four concepts. It
preserved exact sources, revisions, superseded proposals, and human authorship
well.

Discovery was weaker. Search operated over current concept labels and paths,
not evidence or retained work text. `frame` found the parent and child, while
literal evidence phrases such as `interactional convergence` did not. Corpus
quality therefore depended heavily on the vocabulary chosen for labels.

The three roots are also conceptually related, but only the
`Frame construction → Feedback-loop risk` relationship was encoded.

## Validation and limits

Before removal, the database validated cleanly, had a current search index and
no pending changes, occupied 319,488 bytes, and had SHA-256
`6f6d690d1efe5ac94270f12278608477680cae86dda9e49ff923dd0ed227d368`.

This was one deliberately selected three-work trajectory, not a replicated
benchmark. Later works inherited earlier accepted structure, and substantive
human judgment shaped one of the three commits. The database file and private
rendered transcripts are intentionally not retained.
