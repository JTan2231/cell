# Annals experiments

This directory preserves the durable record of four exploratory Annals
ingestion experiments. Each experiment has a walkthrough describing its
inputs, procedure, corpus, evidence, human interventions, results, and
limitations.

| Experiment | Question | Durable artifacts |
| --- | --- | --- |
| [01 — Three-chat medium baseline](01-three-chat-medium/walkthrough.md) | Can a small, coherent conversation set become a useful evidence-linked corpus? | Walkthrough |
| [02 — Three-chat high rerun](02-three-chat-high/walkthrough.md) | How does the higher-grade liaison differ on the identical three works? | Walkthrough |
| [03 — Twenty-chat medium/high comparison](03-twenty-chat-medium-high/walkthrough.md) | Do those differences persist in a larger autonomous trajectory? | Walkthrough, source manifest, locked manifest, runner |
| [04 — Reconciliation-v2 medium/high comparison](04-twenty-chat-medium-high-reconciliation-v2/walkthrough.md) | How do the presets behave on the same 20 works under the best-current-reconciliation contract? | Walkthrough, source manifest, locked manifest, runner |

## Preservation policy

The repository intentionally does not retain SQLite databases, WAL/SHM
sidecars, rendered private transcripts, model logs, setup output, or
snapshotted binaries. Those artifacts were inspected before removal; the
walkthroughs preserve the important corpus structures, metrics, provenance,
interpretations, and caveats. The local ignore file prevents experiment
databases from being added accidentally.

The 20-chat experiments keep two manifests:

- `manifest.json` is the rerunnable source selection.
- `manifest.lock.json` records the exact rendered-input hashes, sizes, session
  metadata, and source hashes from the completed run.

Paths beginning with `~/.codex/sessions` refer to the local Codex session
archive. No source transcript is copied into this repository.

Experiments 3 and 4 intentionally use the same source and locked manifests.
That makes their input cohort identical, but not the rest of their treatment.

## Historical runners

Each 20-chat runner is preserved unchanged with the experiment it produced.
Experiment 3 targets the earlier proposal/outcome/uncertainty contract and
cannot run against the reconciliation schema. Experiment 4 targets
`liaison-v2` and the reconciliation/annotation contract at source commit
`59bf8658e97e886c8881396ac37383c054c3ce17`. A future comparison should use a
new experiment directory and runner so its method and artifacts are not
conflated with either historical run.
