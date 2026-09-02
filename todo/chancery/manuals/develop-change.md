# Change Todo

Todo preserves distinct durable layers:

- `cN`: caller direction and originating provenance;
- `rN`: one pending routing proposal and later explicit decision;
- `tN`: stable enduring concern identity and direction revisions;
- `aN`: immutable dated observed-state assessment;
- `dN`: proposed or accepted normative desired state; and
- `nN`: immutable working note.

Do not collapse those layers into a task planner, implementation workflow, or
general project graph. Nucleus jobs are research provenance, not execution of
the todo.

## Ownership and shared execution

Todo owns its domain records, database, liaisons, explicit decisions, email
projection, and installation. Nucleus owns shared execution, authentication,
job state, and raw protocol history. A model can propose routing, assessment,
or design records only through stage-specific tools. It can never accept or
reject routing/design or assert implementation.

Before changing requester tools, execution, authentication, job records,
compatibility, persistent cross-system state, deployment, or integration, run:

```sh
/Users/joey/.local/bin/nucleus manual
```

Immutable Nucleus schemas and toolsets receive new versions when meaning
changes; historical registrations and decoders remain intact.

## Development workflow

1. Identify the owning layer and exact current contract in `todo/docs/cli.md`,
   `liaison.md`, `architecture.md`, `data-model.md`, or
   `system-installation.md`.
2. Preserve model non-authority, explicit decision provenance, frozen bases,
   stale checks, and historical immutability.
3. Make the smallest implementation and documentation change.
4. Run:

   ```sh
   cd /Users/joey/rust/cell
   ./todo/ci.sh
   ```

   Treat it as the complete product gate.
5. Run separately authorized routing, assessment, design, migration, email, or
   deployment canaries only when their exact boundary changed.

Persistent-state work needs representative old-state fixtures, transactional
migration, a complete backup, and deployment rollback proof. Requester changes
must test duplicate handling, pending-mailbox recovery, daemon loss, and the
case where Todo commits a domain result before later runtime failure.

Model stages intentionally have no shell, workspace, inherited environment,
or web search. Source content is untrusted input exposed only through frozen
managed tools. Do not broaden that permission boundary accidentally.

`todo/release.sh` bumps the package, runs CI, commits, tags, and pushes. It is a
publication command and is not authorized by a development request. Deployment
and installed database migration are separate actions too. Email can disclose
every open todo title and must never be used as an incidental canary.

Databases, backups, decision-source paths, source catalogs, Nucleus output,
email content, API setup, and logs may contain private directions and system
state. Keep every fixture and diagnostic inside the strongest applicable
retention boundary.
