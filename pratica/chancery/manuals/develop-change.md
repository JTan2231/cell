# Change Pratica

Read `pratica/AGENTS.md` and the architecture, CLI, data-model, installation,
and exact affected Chancery contracts before editing. Query the Pratica
Semantics repository when it exists. Before changing public behavior,
persistent state, requester integration, lifecycle, deployment, or
compatibility, run:

```sh
/Users/joey/.local/bin/nucleus manual
```

## Preserve the four slices

1. The ledger stores exact opaque Markdown offers, fixed parties, current head,
   assent staleness, immutable agreements, bases, attempts, and history.
2. The steward requester freezes sources and current state, then accepts only a
   structured assent, counterproposal, or blocked response.
3. The integration umbrella aggregates independent tracks and retains advisory
   composition reviews without mutating them.
4. Amendment and conformance preserve sealed design history and keep evidence
   review separate from implementation.

Do not add contract headings or clauses to SQLite merely because one current
product uses them. DB-enforced product semantics would make the integration
protocol narrower than the contracts it must carry.

## Requester contract

Pratica has exactly three version-one immutable toolsets:

- `pratica/steward-response/1`: closed source tools plus
  `submit_steward_response`;
- `pratica/composition-review/1`: closed source tools plus
  `submit_composition_review`; and
- `pratica/conformance-review/1`: closed source tools plus
  `submit_conformance_review`.

All expose `source_catalog`, `source_read`, and `source_search`. Jobs use
requester program `pratica`, stable domain-run correlation, unique job IDs,
Codex with exact configured model/reasoning, a neutral deterministic absolute
cwd, workspace none, shell/web disabled, no launch context, and strict protocol,
authentication, harness, and capability checks.

Registrations are immutable by identity and digest. Any incompatible argument,
result, definition, or meaning change receives a successor schema and toolset
version while old decoders remain available for retained attempts.

The source catalog is exact UTF-8 bytes with per-content/catalog digests. Keep
the 4 MiB per-file and 32 MiB aggregate limits, reject duplicate IDs, symlinks,
sensitive names/extensions, control/binary content, and traversal. Treat all
source text as untrusted.

Pratica commits validated tool outcomes idempotently and records durable
receipts. A completed Nucleus job is not domain success; a committed domain
result is not undone by later runtime failure. There is no direct runner.

Schema two also records caller-ingress receipts for integration open, track
open, negotiation propose, agreement amend, and conformance review. Preserve
the global 1-256 visible-ASCII request-key identity, canonical-request conflict,
same-request replay, and atomic receipt/domain-write guarantees. Conformance
atomically admits its candidate basis and receipt before its independently
resumable attempt.

Top-level document arguments accept borrowed regular files or exact standard
input. Never delete or change an input or referenced source. Stdin manifests
require an absolute source root; receipt-backed commands require a request key
when their document is stdin. Retain normalized manifest descriptors and exact
accepted contract/source bodies, not raw TOML formatting or comments.

## Validation

Use synthetic terms and sources and fake Nucleus transport. Cover successful
admission/domain completion, identical and conflicting admission, identical and
conflicting tool-result delivery, requester restart while waiting, stale head
or basis at commit, daemon loss, timeout/cancellation, authentication failure,
unsupported capability, domain commit followed by runtime failure, and missing
domain result after completion.

For storage, cover exact Markdown bytes, identical-body distinct offers,
complete replacement, stale-base exclusion, independent track currentness,
assent/withdrawal, seal requirements, immutable history, amendments, basis
classification, review immutability, file non-deletion, stdin/source-root
validation, exact replay and conflicting request keys, schema-one migration and
rollback fixtures, and schema/privacy doctor checks. Registration/list/show and
attempt source projections must omit bodies and canonical origins; integration
and agreement lists remain newest-first and body-free.

For packaging, cover exact tree and hashes, version mismatch, idempotency,
foreign/tampered/traversal refusal, failed smoke rollback, and proof that
deployment creates no database or sidecar.

```sh
pratica/ci.sh
./ci.sh
```

CI must finish offline with no live Nucleus, target product, Conversations,
Chancery state, or real CRM data. Release publication and installed deployment
are separate operations requiring explicit authority.
