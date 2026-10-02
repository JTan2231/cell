# Mantic

Semantics-Project: mantic

- Read the registered Mantic Semantics repository before product analysis,
  review, or changes. Source, tests, and published contracts own runtime facts.
- Keep Mantic a local calculation engine. Persist only the current named
  configs and their expense items in one private database. Each item belongs
  to one config. Forecast inputs and results are transient.
- Keep amounts in integer cents. Generate calendar recurrences from the
  original first due date. Preserve inclusive forecast dates and end cutoffs.
- Keep forecast reads free of product writes. Do not retain balances, runs,
  payment status, generated occurrences, or an advancing next due date.
- Keep private configuration and forecast output outside the source tree.

## Documentation ownership

- Keep full feature explanations in `chancery/manuals/`. Update the matching
  normalized claims in `chancery/entries/` when meaning changes.
- Keep the product overview in `chancery/overview.md`. Keep the README to
  purpose, a small example, and documentation pointers.
- Publish the complete bundle with the matching release. Preserve compatible
  entry identities and versions. Follow the Cell CI submission instructions.
