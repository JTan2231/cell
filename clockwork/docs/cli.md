# Clockwork commands

The installed feature contracts own command formats, data meaning, effects,
failures, and supported limits. Read a focused page with `chancery show ID`.
Use `chancery resolve clockwork.schedule.operate` for the complete operating
procedure and its required explanations.

| Commands | Authoritative page |
| --- | --- |
| `definition register`, `list`, `show` | [Definitions](../chancery/manuals/definitions.md) · `clockwork.definitions` |
| `binding switch`, `disable`, `list`, `show` | [Bindings](../chancery/manuals/bindings.md) · `clockwork.bindings` |
| `run`, `history`, `doctor`, `status-snapshot` | [Activations](../chancery/manuals/activations.md) · `clockwork.activations` |
| `abend`, `binding halt`, `binding resume`, `incident list`, `show`, `feed` | [Incidents](../chancery/manuals/incidents.md) · `clockwork.incidents` |
| `notification policy`, `check`, `send`, `retry`, `emt`, `show`, `claim` | [Notifications](../chancery/manuals/notifications.md) · `clockwork.notifications` |
| Rust installer and `migrate` | [Installation](../chancery/manuals/installation.md) · `clockwork.installation` |

Use [schedule operation](../chancery/manuals/schedule-operate.md) for ordered
registration, selection, inspection, continuation, alert, and migration steps.
Use [installation operation](../chancery/manuals/install-operate.md) for program
installation, diagnosis, rollback, and selector detach.
