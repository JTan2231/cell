# Clew

Semantics-Project: clew

- Query Clew's Semantics repository for maintained terminology. Source and tests
  define runtime behavior.
- Clew owns user-reported application history. Cast owns job identities and
  retained job details. Use Cast's public snapshot interface. Platter owns
  prepared material; its public reader supplies legacy migration mappings only.
- Append status and notes only on the user's instruction. Never infer status from
  postings, delivery, correspondence, or elapsed time.
- Preserve ledger entries. Corrections append replacements or retractions.
- Preserve exact write identity and legacy reference aliases during migration.
- Keep private application notes outside the repository.
