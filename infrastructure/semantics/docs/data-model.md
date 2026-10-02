# Data model

The feature contracts own the meaning and consistency of Semantics records.
Direct SQLite access is unsupported. Read `chancery show ID` for the installed
feature; use the [development procedure](../chancery/manuals/develop-change.md)
when changing persistent state.

## Project registry

Read [project identity, activation, and lifecycle](../chancery/manuals/projects.md).

## Repository

Read [concepts, typed effects, and immutable replay](../chancery/manuals/repository-explore.md).

## Intake and reconciliation

Read [source identity, intake state, requests, and receipts](../chancery/manuals/reconciliation.md).

## Migration

Read [persistent compatibility and feed cutover](../chancery/manuals/service.md#persistent-compatibility-and-feed-cutover)
and [rollback guarantees](../chancery/manuals/service.md#rollback-and-recovery).
Use the [migration and recovery procedures](../chancery/manuals/project-operate.md)
for prerequisites, actions, and verification.
