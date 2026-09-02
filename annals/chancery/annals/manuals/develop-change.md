# Change Annals

Annals development begins with its canonical Semantics repository, which
defines both technical and conversational terms:

```sh
/Users/joey/.local/bin/chancery show semantics.repository.explore
/Users/joey/.local/bin/semantics repository show annals
```

Read the repository before analyzing or changing Annals code, tests,
documentation, or interfaces. Code, tests, and component documentation remain
authoritative for actual behavior. Semantics repository output is contributor
guidance and must never be added to the constrained liaison's prompt or runtime
context.

## Ownership

Annals owns retained works, source deliveries, concepts, evidence,
reconciliations, revisions, inbox policy, retries, domain recovery, and its
installed state. Nucleus owns shared Codex execution and authentication.
Nucleus completion is not Annals success, and Nucleus must not acquire Annals
workflow fields or retry policy.

Before changing the requester contract, shared execution, authentication, job
records, compatibility, persistent cross-system state, deployment, or another
cross-system integration, read:

```sh
/Users/joey/.local/bin/nucleus manual
```

## Development workflow

1. Identify the owning Annals contract: CLI, architecture, data model, search,
   telemetry, installation, Semantics terminology, or preserved historical
   experiment.
2. Make the smallest change without collapsing work, delivery, examination,
   reconciliation, commit, and revision lifecycles.
3. Update the owning documentation when public or operational meaning changes.
4. Run the product gate:

   ```sh
   cd /Users/joey/rust/cell
   ./annals/ci.sh
   ```

   Treat it as the complete Annals product gate.
5. Run separately authorized deployment, migration, and live requester
   canaries only when the changed boundary requires them.

Keep current Semantics terminology out of the preserved experiment archive, whose older
tree, path, placement, proposal, and uncertainty terms are deliberately
historical. Conversely, do not revive historical terms in current contracts.

## Compatibility and recovery

The Annals release, library schema, Nucleus public protocol, immutable
tool/schema registrations, and installed packaging are separate compatibility
axes. Define migration and rollback whenever persistent meaning changes.
Preserve exact domain results after later runtime failure and retain historical
decoders where Nucleus records require them.

Installation changes must respect scheduler quiescence, maintenance,
transactional database backup, release selector rollback, and forward-only
Nucleus authentication. Never edit the installed database or spool as an ad
hoc migration.

`annals/release.sh` commits, tags, and pushes; it is a publication command, not
a test command. Development completion does not authorize it or installation.

Source fixtures, experiment archives, libraries, spools, Nucleus output,
backups, and live canaries can contain private source and model context. Treat
each according to the strongest content it may retain.
