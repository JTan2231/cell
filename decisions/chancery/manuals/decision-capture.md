# Capture decision accounts

Use `krisis observe status`, `process`, and `reconcile` to operate automatic
capture. Each eligible user authority in a completed root turn receives a
durable binary verdict. Krisis delivers deterministic Markdown decision accounts
to the configured dedicated Annals library.

Krisis exposes observation status and account delivery. Pending Annals delivery
retries exact bytes before new classification. Search accepted accounts through
Annals.

See `docs/architecture.md` and `docs/cli.md` in the Krisis source distribution
for the full source, classification, transaction, and recovery boundaries.

The provider-owned `krisis-api` crate defines schema-one account sections,
source metadata, authority anchors, and the canonical Markdown codec. Krisis
uses it to render its accounts; Annals uses it to decode incoming content.
Acceptance requests and receipts use Annals' own `annals-api` client and types.
These libraries preserve the existing wire and durable byte identities.

Rust operational callers use `decisions::api::Client` with exported hook input,
activation, processing, status, reconciliation, and diagnostic types. Methods
invoke the same explicit CLI operations and retain their effects and authority
requirements. Retired digest/review commands are not available in this client.

## Output selection

Activation, hook, process, and Annals acceptance receipts retain their documented
identities and meanings. Observer status returns all counts and, by default,
at most 20 failure IDs and codes. It includes `failures_has_more`. To see more
failures, set `--limit` to a larger positive integer. Full source-account and
legacy lifecycle reads preserve their schemas and authority.
