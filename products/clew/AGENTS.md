# Clew

Semantics-Project: clew

- Query Clew's Semantics repository for maintained terminology. Source and tests
  define runtime behavior.
- Clew owns supplied ledger statements, optional named threads, external
  reference associations, and explicit application reports. External references
  are optional links to stable identities outside Clew. Milieu owns opportunity identities
  and retained opportunity details. Use Milieu's public snapshot interface. Platter owns
  prepared material; its public reader supplies legacy migration mappings only.
- Append supplied status and notes only on the user's instruction. Never infer
  status or completion from postings, delivery, correspondence, elapsed time,
  or external links. Plain Milieu opportunity links are not application reports.
- Preserve ledger entries. Corrections append replacements or retractions and
  inherit their target's thread. Named threads hold only an ID and unique name.
- Preserve exact write identity and legacy reference aliases during migration.
- Keep private notes, statuses, thread names, and references outside the repository.

## Documentation ownership

- Keep full feature explanations in `chancery/manuals/`. Update their normalized
  claims in `chancery/entries/` when meaning changes.
- Keep the product overview in `chancery/overview.md`. Keep prerequisites, action
  steps, consequential effects, stop conditions, and verification in operation
  manuals. Declare required feature contracts as bounded dependencies.
- Keep README text to purpose, a small example, and documentation entry points.
  Publish the complete provider bundle with the matching release. Preserve
  compatible entry identities and versions.
