# Capture decision accounts

Use `krisis observe status`, `process`, and `reconcile` for the automatic capture
pipeline. Every eligible completed root-turn user authority receives a durable
binary verdict. Decisions become deterministic Markdown accounts delivered to
the explicitly configured dedicated Annals library.

Krisis does not expose a digest, review queue, confidence, supersession, current
force, or account browser. Pending Annals delivery retries exact bytes before
new classification. Search accepted accounts through Annals.

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

Activation, hook, process and Annals acceptance receipts retain their documented identity and domain meaning. Observer status retains complete counts and at most 20 failure IDs/codes by default, with failures_has_more and positive --limit for more. Full source-account and legacy lifecycle reads preserve their existing schemas and authority.
