# Rust API

Bazaar's feature contracts own the in-process Rust API and its guarantees:

- Read [strings and version history](../chancery/manuals/string-read.md) for
  `Reader`, `Record`, query results, and read failures.
- Read [append-only updates](../chancery/manuals/string-update.md) for `Writer`,
  transactions, update receipts, and uncertain completion.
- Read [installation and private state](../chancery/manuals/installation.md) for
  `Writer::initialize`, `Reader::check`, path rules, and state recovery.

Read the installed pages with `chancery show bazaar.string.read`,
`chancery show bazaar.string.update`, or `chancery show bazaar.installation`.
The [product overview](../chancery/overview.md) explains how these features fit
together. The [operating procedure](../chancery/manuals/install-operate.md) owns
installation, initialization, verification, and recovery steps.
