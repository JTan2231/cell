# CRM agent instructions

Semantics-Project: crm

- Keep CRM simple: it is one private local SQLite library, a short-lived CLI,
  and a hidden per-update worker. It is not a daemon, scheduler, contact
  sender, source crawler, general automation system, or filesystem content
  repository.
- Until the `crm` Semantics project is explicitly registered and seeded, use
  the Cell semantic repository for shared terminology. After registration,
  the CRM project repository is authoritative for maintained CRM terminology.
  Code, tests, and product documentation remain authoritative for behavior.
  Never edit Semantics state directly.
- Store every retained intake and Markdown body as SQLite `TEXT`. A caller's
  input file or standard-input stream is transient transport; never create a
  parallel tree of product-owned content files.
- CRM owns case identity, immutable revisions, stage, advisory text, intake
  deliveries, update state, requester attempts, and tool receipts. A cited
  source remains authoritative for the fact it supplies, and Nucleus remains
  authoritative only for agent execution and mailbox transport.
- An advisory must remain conspicuous on every surface that consumes its
  revision, but it must never authorize, refuse, or block an operation. CRM
  records reasoning and evidence; it does not contact anyone or prove an
  external relationship by itself.
- `tell` must durably record the delivery and queued update before returning.
  The hidden worker uses only `crm/case-steward/1`; there is no scheduler,
  automatic retry, or direct-Codex fallback.
- Preserve immutable case revisions and replay-safe tool receipts. A retry is
  an explicit new attempt with a new Nucleus job identity.
- Update architecture, CLI, data-model, installation, packaging, tests, and
  Chancery contracts together when their shared behavior changes.
- `release.sh` commits, tags, and pushes, and the macOS deployer changes
  installed selectors. Do not invoke either without separate authority.
- Every code change must leave `./ci.sh` green.
