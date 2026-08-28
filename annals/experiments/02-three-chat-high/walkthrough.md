# Experiment 2: Three-chat high-quality rerun

## Question

How would the liaison handle the identical three-work corpus after switching
from the medium-grade configuration to the higher-grade model?

The higher-grade liaison investigated more, cited more evidence, and integrated
later material into a deeper structure. It also took much longer and expressed
more uncertainty, so human review contributed more to the final corpus.

## Controlled setup

The run began with a fresh revision-zero library and used the same inputs,
order, byte content, hashes, and `liaison-v1` prompt as experiment 1. Total
retained input was 79,038 bytes.

| Experiment | Model | Reasoning effort |
| --- | --- | --- |
| Medium baseline | `gpt-5.6-terra` | `medium` |
| High rerun | `gpt-5.6-sol` | `max` |

Both model family and reasoning effort changed, so this was a quality-preset
comparison—not an effort-only test.

## Cost and investigation

| Work | Medium time / calls | High time / calls |
| --- | ---: | ---: |
| Conversation ontology | 20.9 s / 6 | 139.2 s / 16 |
| Year interpretation | 43.1 s / 7 | 96.0 s / 12 |
| Frame construction | 21.0 s / 6 | 60.1 s / 10 |
| **Total** | **85.0 s / 19** | **295.3 s / 38** |

High took 3.47 times as long and made twice as many tool calls. It performed 17
corpus searches versus medium's 6 and 9 work reads versus medium's 5. Three
high calls and one medium call failed with recoverable tool-input errors.

## Raw model behavior

### Conversation ontology

Medium proposed `Fork-aware conversation views` with two quotations. High
proposed `Fork-aware conversation indexing`, also with two quotations, but its
evidence covered both representation and downstream counting: one canonical
message tree, separately classified branch views, reusable shared-prefix
summaries, and family-level aggregation to prevent fork-counting bias.

Both were certain and applied directly.

### Interpreting a year

Medium proposed `Archive-use analysis` with one quotation capturing the
usage-map-versus-biography boundary.

High proposed `Usage-centered conversation archive analysis` with three
quotations. It retained a broader method:

- study what the user asks the model to do, not only the subject matter;
- corroborate shifts across several archive signals;
- limit conclusions to archive use rather than biography.

High also surfaced limits on generalizability, fork-inflated activity counts,
and unresolved images. Review retained its root and all three quotations, then
promoted the concrete measurement limitation into an evidenced child,
`Archive measurement caveats`.

### Frame construction

Medium had treated the final work as a new root and left feedback-loop risk in
uncertainty metadata until human review promoted it into a child.

High instead related both findings to the existing archive-analysis tree:

```text
Usage-centered conversation archive analysis
├── Archive measurement caveats
│   └── Reuse-induced interactional convergence
└── AI-assisted frame construction
```

It distinguished the positive finding—conversation externalizes latent
structure and produces reusable frames—from the warning that repeated language
may reflect reuse-driven convergence rather than independent confirmation.

The model submitted one epistemic uncertainty. Human review treated that as
the scope of the claim rather than a reason to block it. The replacement kept
both model-proposed operations, paths, and quotations unchanged.

## Human review

| Metric | Medium | High |
| --- | ---: | ---: |
| Model proposals | 3 | 3 |
| Uncertain model proposals | 1 | 2 |
| Human replacement proposals | 1 | 2 |
| Model-authored commits | 2 | 1 |
| Human-authored commits | 1 | 2 |
| Total stored proposals | 4 | 5 |

The higher-grade final tree was therefore co-produced. In particular, its
excellent third-work placement depended on `Archive measurement caveats`,
which human review had added after work 2. Raw model proposals and the final
reviewed corpus should not be conflated.

## Final reviewed corpus

```text
Fork-aware conversation indexing [2 evidence links]

Usage-centered conversation archive analysis [3]
├── Archive measurement caveats [1]
│   └── Reuse-induced interactional convergence [1]
└── AI-assisted frame construction [1]
```

| Metric | Medium | High |
| --- | ---: | ---: |
| Works / revision | 3 / 3 | 3 / 3 |
| Concepts | 4 | 5 |
| Roots | 3 | 2 |
| Maximum depth | 2 | 3 |
| Evidence links | 5 | 8 |
| Stored proposals | 4 | 5 |
| Pending proposals | 0 | 0 |

High separated two broad concerns: representing forked conversations and
interpreting a longitudinal archive. It then encoded method, measurement
limits, frame construction, and reuse-induced convergence inside one archive
analysis hierarchy. Medium left the three works mostly as separate roots.

Only two final quotations were exact matches across the corpora: the
usage-versus-biography conclusion and the feedback-loop warning. Different
quotation selection did not always mean conceptual disagreement; the two
models sometimes selected adjacent sentences from the same central passage.

## Interpretation

Medium behaved like a selective summarizer. High behaved more like a corpus
editor: it searched harder, preserved more of each work's reasoning,
distinguished findings from limitations, and used prior concepts when placing
new material.

The deeper hierarchy was useful but not indisputably correct. Nesting
`Reuse-induced interactional convergence` under `Archive measurement caveats`
connects it to methodological risk, while medium's direct
`Frame construction → Feedback-loop risk` relationship may express the local
causal connection more clearly.

High's uncertainties were intellectually valuable, but the workflow initially
treated scope qualifications as blockers even when they could be represented
as concept scope or caveat nodes. This became a central question for the larger
experiment.

## Validation and limits

Before removal, the high database validated cleanly, occupied 450,560 bytes,
and had SHA-256
`814284b8a4d12dacf97f9ce988aaf7f1aaf656a207c309a713aa767689bea7ee`.

This was a three-work exploratory comparison with no replication. Ingestion was
sequential, so later runs saw reviewed earlier state. Much of the evidence was
assistant-authored, and evidence proves what the retained conversation said,
not external correctness. The database file and private rendered transcripts
are intentionally not retained.
