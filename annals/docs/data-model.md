# Annals records

The published feature contracts explain current record identity, meaning,
consistency, and recovery:

| Records | Authoritative explanation |
| --- | --- |
| Catalog, library profile, instruction revisions, and backups | [Libraries](../chancery/annals/manuals/libraries.md) |
| Immutable works, source deliveries, and lifecycle times | [Retained sources](../chancery/annals/manuals/work-retain.md) |
| Concepts, explicit edges, evidence occurrences, and replay-backed reads | [Corpus reading](../chancery/annals/manuals/corpus-explore.md) |
| Model runs, tool audit, and reconciliation drafts | [Interpretation](../chancery/annals/manuals/work-integrate.md) |
| Typed request intent, reconciliations, commits, and canonical effects | [Corpus changes](../chancery/annals/manuals/corpus-change.md) |
| Job receipts, retry events, frozen items, and child provenance | [Inbox](../chancery/annals/manuals/inbox.md) |
| Producer acceptances, library binding, and document-feed cursors | [Document exchange](../chancery/annals/manuals/decision-account-exchange.md) |
| Disposable consumption projection | [Annals Usage](../chancery/annals-usage/manuals/consumption-inspect.md) |

The [SQLite schema](../crates/annals/schema.sql) describes storage declarations
for contributors. Use the feature contracts for the supported integration
boundary. Read installed features with `chancery show ID`; use `chancery resolve
ID` to include required contracts and compatibility gaps.
