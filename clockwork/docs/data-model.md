# Clockwork records

Feature contracts own the meaning, identity, lifecycle, consistency, privacy,
and supported reading of Clockwork records. A SQLite schema describes stored
shape; it is not a public integration interface.

| Records | Authoritative explanation |
| --- | --- |
| Immutable definition content and digest | [Definitions](../chancery/manuals/definitions.md) · `clockwork.definitions` |
| Stable selection, plist ownership, and transition journals | [Bindings](../chancery/manuals/bindings.md) · `clockwork.bindings` |
| Activation identity, process state, timestamps, history, and status | [Activations](../chancery/manuals/activations.md) · `clockwork.activations` |
| Deduplicated abends, halts, approvals, and insertion cursors | [Incidents](../chancery/manuals/incidents.md) · `clockwork.incidents` |
| Transport status, check progress, routing, and delivery claims | [Notifications](../chancery/manuals/notifications.md) · `clockwork.notifications` |
| Store compatibility, backup, migration, and retained-state scope | [Installation](../chancery/manuals/installation.md) · `clockwork.installation` |

Read installed content with `chancery show ID`. Use
`chancery resolve clockwork.install.operate` for recovery procedures and required
contracts. Keep runtime state and backups private. Do not edit SQLite directly.
