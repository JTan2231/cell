# Experiment 2: Three-chat high-quality rerun

## Question

How would the liaison handle the identical three-work corpus after switching
from the medium-grade configuration to the higher-grade model?

The high preset made more tool calls, selected more quotations, and integrated
later material into a deeper structure. It took longer and populated
`uncertainties` on more proposals, so more proposals went through human review.

## Controlled setup

The run began with a fresh revision-zero library and used the same inputs,
order, byte content, hashes, and `liaison-v1` prompt as experiment 1. Total
retained input was 79,038 bytes.

| Experiment | Model | Reasoning effort |
| --- | --- | --- |
| Medium baseline | `gpt-5.6-terra` | `medium` |
| High rerun | `gpt-5.6-sol` | `max` |

Both model family and reasoning effort changed. The experiment therefore
compared quality presets, not reasoning effort alone.

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

Both had empty `uncertainties` arrays and were applied directly.

### Interpreting a year

Medium proposed `Archive-use analysis` with one quotation capturing the
usage-map-versus-biography boundary.

High proposed `Usage-centered conversation archive analysis` with three
quotations. It retained a broader method:

- study what the user asks the model to do alongside the subject matter;
- compare shifts across several signals recorded in the archive;
- describe the uses recorded in those conversations.

High also represented the source's discussion of generalizability,
fork-inflated activity counts, and images. Review retained its root and all
three quotations, then represented the measurement discussion in an evidenced
child, `Archive measurement caveats`.

### Frame construction

Medium had treated the final work as a new root and placed feedback-loop risk
in the `uncertainties` field until human review represented it in a child.

High instead related both findings to the existing archive-analysis tree:

```text
Usage-centered conversation archive analysis
├── Archive measurement caveats
│   └── Reuse-induced interactional convergence
└── AI-assisted frame construction
```

It represented conversation as a way to externalize latent structure and
produce reusable frames, alongside the role of language reuse in recurring
vocabulary.

The model recorded one entry in `uncertainties`. Human review expressed that
entry in the concept scope. The replacement kept both model-proposed
operations, paths, and quotations unchanged.

## Human review

| Metric | Medium | High |
| --- | ---: | ---: |
| Model proposals | 3 | 3 |
| Model proposals with `uncertainties` entries | 1 | 2 |
| Human replacement proposals | 1 | 2 |
| Model-authored commits | 2 | 1 |
| Human-authored commits | 1 | 2 |
| Total stored proposals | 4 | 5 |

Human edits contributed to the final high-preset tree. Its third-work placement
depended on `Archive measurement caveats`, which review added after work 2.
The raw model proposals therefore differ from the final reviewed corpus.

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

Only two final quotations matched exactly across the corpora: the
usage-versus-biography conclusion and the feedback-loop warning. The models
sometimes selected adjacent sentences from the same passage. Different
quotation selection therefore did not always indicate conceptual disagreement.

## Interpretation

Medium selected a small set of concepts from each work. High made more
searches, preserved more of each work's reasoning, distinguished findings from
limitations, and used prior concepts when placing new material.

The two runs encoded different relationships. High nested
`Reuse-induced interactional convergence` under `Archive measurement caveats`,
connecting it to methodological risk. Medium used the direct
`Frame construction → Feedback-loop risk` relationship.

High's `uncertainties` entries described source qualifications. The workflow
withheld those proposals under its application policy. Review subsequently
represented the qualifications in concept scope and caveat nodes; the larger
experiment examined this interaction between recorded fields and application.

## Validation and limits

Before removal, the high database validated cleanly, occupied 450,560 bytes,
and had SHA-256
`814284b8a4d12dacf97f9ce988aaf7f1aaf656a207c309a713aa767689bea7ee`.

This was a three-work exploratory comparison with no replication. Ingestion
was sequential, so later runs saw reviewed earlier state. Much of the selected
source text was assistant-authored. The database file and private rendered
transcripts are intentionally not retained.
