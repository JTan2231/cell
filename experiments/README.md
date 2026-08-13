# Annals experiments

This directory preserves the durable record of three exploratory Annals
ingestion experiments. Each experiment has a walkthrough describing its
inputs, procedure, corpus, evidence, human interventions, results, and
limitations.

| Experiment | Question | Durable artifacts |
| --- | --- | --- |
| [01 — Three-chat medium baseline](01-three-chat-medium/walkthrough.md) | Can a small, coherent conversation set become a useful evidence-linked corpus? | Walkthrough |
| [02 — Three-chat high rerun](02-three-chat-high/walkthrough.md) | How does the higher-grade liaison differ on the identical three works? | Walkthrough |
| [03 — Twenty-chat medium/high comparison](03-twenty-chat-medium-high/walkthrough.md) | Do those differences persist in a larger autonomous trajectory? | Walkthrough, source manifest, locked manifest, runner |

## Preservation policy

The repository intentionally does not retain SQLite databases, WAL/SHM
sidecars, rendered private transcripts, model logs, setup output, or
snapshotted binaries. Those artifacts were inspected before removal; the
walkthroughs preserve the important corpus structures, metrics, provenance,
interpretations, and caveats. The local ignore file prevents experiment
databases from being added accidentally.

The 20-chat experiment keeps two manifests:

- `manifest.json` is the rerunnable source selection.
- `manifest.lock.json` records the exact rendered-input hashes, sizes, session
  metadata, and source hashes from the completed run.

Paths beginning with `~/.codex/sessions` refer to the local Codex session
archive. No source transcript is copied into this repository.

## Historical runner

The 20-chat runner is preserved with the experiment it produced. It targets
the earlier proposal/outcome/uncertainty contract and its database schema; it
does not run against the current reconciliation contract. The locked manifest,
runner, and walkthrough remain available to audit that historical method. A
new comparison should use a new experiment directory and runner so its results
are not conflated with experiment 03.
