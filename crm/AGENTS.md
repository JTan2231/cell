# CRM agent instructions

Semantics-Project: crm

- Keep CRM simple: one private SQLite library, a short-lived CLI and a hidden
  worker for each update. Do not add a daemon, scheduler, contact sender,
  source crawler, general automation system or filesystem content repository.
- Until the `crm` Semantics project is explicitly registered and seeded, use
  the Cell semantic repository for shared terminology. After registration,
  the CRM project repository is authoritative for maintained CRM terminology.
  Code, tests, and product documentation remain authoritative for behavior.
  Never edit Semantics state directly.
- Store every retained intake and Markdown body as SQLite `TEXT`. A caller's
  input file or standard-input stream is transient transport; never create a
  parallel tree of product-owned content files.
- CRM owns case identity, immutable revisions, stage, advisory text, intake
  deliveries, update state, requester attempts, and tool receipts. Source
  references are retained with deliveries. Nucleus owns agent execution and
  mailbox transport.
- Display an advisory prominently on every surface that consumes its revision.
  An advisory must never authorize, refuse or block an operation. CRM
  stores the case narrative, stage and supporting material; contact actions
  belong to the caller.
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
