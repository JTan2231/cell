# Search relevance fixture

`relevance.jsonl` records the version-one query classes and expected IDs for a
small deterministic library. The relevance test constructs the complete seed,
including the punctuation-heavy technical title at ID 6, and compares every
judgment before ranking weights are changed.

The scored cases cover duplicate titles, normalized full and scoped paths,
quoted phrases, a missing-term OR fallback, punctuation and identifier
tokenization, title-prefix fallback, kind and subtree filters, several sibling
branches, overlapping long passages, and direct matches on adjacent ancestor
levels.

The release gate is the one defined in `docs/implementation-plan.md`: exact
topic and distinctive phrase cases appear in the first three results,
`Recall@10` is at least 0.90, and grouping does not reduce scoped-query recall.
The test additionally reports `Recall@5`, `Recall@10`, mean reciprocal rank,
preferred-primary presence, scoped recall, and required-branch presence. It
requires `Recall@5 >= 0.80`, `MRR >= 0.75`, scoped `Recall@10 >= 0.90`, and all
recorded preferred primaries and required branches to be present.

Typo matching is deliberately deferred by the version-one search design. The
misspelling `Transactons` is therefore recorded as an expected current miss and
is not included in the scored JSONL judgments. Add it only after a measured
failure case justifies a title trigram or other explicit typo fallback.
