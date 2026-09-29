# Annals

Annals retains exact source documents and organizes their interpretation in
named libraries. Each library has its own instructions, concepts, quotations,
and revision history. The retained source remains available when an
interpretation changes.

## How the features fit together

A library selects the source and interpretation boundary. Retention preserves
immutable text. Model integration examines one work against frozen instructions
and corpus state, then records a reconciliation. Explicit application advances
the corpus when the result changes it. Inbox dispatch combines retention,
examination, and immediate application under its queue and failure policy.

A separately provisioned decisions library accepts unchanged Krisis documents
and publishes a fixed-prefix feed of accepted text. Acceptance is available
before dispatch and remains separate from interpretation. Corpus reads expose
concepts, exact supporting quotations, source activity, and revision history.

| Feature | Authoritative page | What it explains |
| --- | --- | --- |
| Libraries and instructions | `annals.libraries` | Selection, identity, configuration, instruction revisions, storage, and backups |
| Retained sources | `annals.work.retain` | Exact bytes, work labels, source deliveries, metadata, and retention outcomes |
| Corpus reading | `annals.corpus.explore` | Concepts, evidence, lexical search, bounded graph views, activity, and history reads |
| Corpus changes | `annals.corpus.change` | Reconciliation language, validation, application, commits, shake, and revert |
| Model interpretation | `annals.work.integrate` | Frozen examination, model policy, requester tools, drafts, reuse, and diagnostics |
| Inbox processing | `annals.inbox` | Queue order, attempts, controls, storage gates, retries, and scheduled failures |
| Decision-document exchange | `annals.decision-account.exchange` | Producer acceptance, unchanged text, identity binding, and bounded feed |
| Installation and maintenance | `annals.installation` | Release selection, deployment admission, platform boundaries, and recovery |

Read one page with `chancery show ID`. Read a procedure and its complete required
contracts with `chancery resolve ID`. The resolver reports compatibility,
readiness, exact source basis, and intentional gaps. It does not run Annals or
prove live readiness.

## Operate Annals

Use `annals.library.operate` to create libraries, select instructions, back up or
migrate state, and explicitly submit, apply, simplify, or revert the corpus.
Use `annals.inbox.operate` to enqueue, dispatch, pause, prioritize, interrupt,
or recover a bounded range of failed deliveries.

Use `annals.install.operate` for macOS installation and recovery, dedicated
decisions provisioning, older-installation migration, or the packaged Linux
systemd route. Use `annals.develop.change` for software and contract changes.
These procedures retain prerequisites, consequential effects, stop conditions,
and verification; feature pages own their detailed behavior.

## Authority and privacy

Annals owns the library catalog, exact instructions and source bytes, accepted
documents, corpus, reconciliations, revisions, inbox outcomes, and installed
state. Library instructions define interpretation; Annals enforces structural
validity. Its graph is not a truth determination or an application-specific
success assertion.

Nucleus owns shared model execution, authentication, jobs, attempts, and raw
output. Clockwork owns macOS scheduled activation and incidents. Annals owns
queue policy and retries. Annals Usage owns a separately versioned live
consumption projection; read `chancery product annals-usage`. An account allowance
is shared account information, not a per-delivery consumption share.

Libraries, catalogs, instructions, source archives, evidence, model context,
logs, and backups can contain private content. Model integration can disclose
source and frozen corpus context through Nucleus to its model provider.
Read access grants no mutation, retry, application, deployment, cleanup, or
remote-sharing authority. Installed documentation grants no additional access.

## Release and compatibility

The provider bundle and overview are sealed with the owning Annals release.
Product version, feature contract version, library schema, Nucleus protocol,
Clockwork definition digest, and installed generation remain separate identities.
Required contract dependencies assemble reading and check compatibility; they
are not runtime calls. Explicit unsupported behavior and unspecified guarantees
remain visible in feature pages and resolver output.

README and older topic pages are navigation. Preserved experiment walkthroughs
remain historical evidence; they are not current feature contracts.
