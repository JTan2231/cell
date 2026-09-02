# Change Semantics safely

Read `semantics/AGENTS.md`, architecture, data model, the affected Decisions and
Conversations contracts, and both Nucleus requester and operator manuals before
changing persistent state, the toolset, service lifecycle, or packaging.

## Invariants

- Project and concept IDs survive moves and wording changes.
- Registration uses the current opaque Decisions watermark; moves preserve
  activation and scan cursors.
- Repository state is replayed from contiguous immutable revisions and typed
  effects. Version one has no mutable concept projection.
- Each effective admission or confirmation grounds its exact event and decision.
  Dismissal appends `unground`; it never deletes provenance.
- Active normalized canonical labels are unique, new concept IDs are strictly
  sequential, and an entire revision validates before any effect commits.
- Nucleus uses a neutral cwd, workspace `none`, no shell, no web, exactly one
  immutable managed tool, and no domain authority. Semantics commits.
- One persisted requester/job identity survives ambiguous submission and result
  transport. Tool receipt plus revision is atomic; identical redelivery is
  idempotent and conflicting redelivery fails.
- Paused projects reject late commits. Worker execution is cross-process serial.
- Chancery is documentation and discovery, never a runtime dependency.

## Testing

Use synthetic Decisions pages and Conversations cwd values. Nucleus integration
tests use the fake local server and immutable schemas; never connect CI to the
live service. Packaging tests use fake candidate binaries and fake launchctl in
an isolated home. Fixtures contain no real user content, credentials, or
personal paths.

```sh
semantics/ci.sh
```

The complete gate is offline. It validates shell and plist syntax,
runner/frontend behavior, content-addressed deployment,
database quiescence and rollback, retained-state uninstall, Chancery provider
and dependency contracts, rustfmt, clippy, tests, rustdoc, and a release build.

For a SQLite schema change, add an explicit versioned migration. Deployment
must stop the worker, suspend the public command, prove the database is closed,
and privately back up the database plus `-wal`, `-shm`, and `-journal` before a
candidate can touch it. Prove rollback after a candidate mutation and a later
activation failure.

For a changed Nucleus tool input, result, instructions, or semantic meaning,
publish new immutable schema IDs or a new toolset version. Do not reinterpret
persisted correlations in place.

Keep project operation docs, Chancery entries/manuals, packaging manifests, and
the shared Nucleus operator manual synchronized with operational changes.
Development does not itself authorize deployment, publication, upstream
mutation, or retained-state deletion.
