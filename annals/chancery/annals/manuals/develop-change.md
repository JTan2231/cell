# Change Annals

Annals development begins with its canonical Semantics repository, which
defines both technical and conversational terms:

```sh
/Users/joey/.local/bin/chancery show semantics.repository.explore
/Users/joey/.local/bin/semantics repository show annals
```

Read the repository before you analyze or change Annals code, tests,
documentation, or interfaces. Code, tests, and component documentation remain
authoritative for actual behavior. Semantics repository output is contributor
guidance and must never be added to the constrained liaison's prompt or runtime
context.

## Ownership

Annals owns its library catalog, stored instruction revisions, retained works, source deliveries, concepts, evidence,
reconciliations, revisions, inbox policy, retries, domain recovery, and its
installed state. Nucleus owns shared Codex execution and authentication.
Clockwork owns immutable scheduled activation, binding, overlap admission, and
process history. Neither service owns Annals queue, retry, or corpus success.

Before changing the requester contract, shared execution, authentication, job
records, compatibility, persistent cross-system state, deployment, or another
cross-system integration, read:

```sh
/Users/joey/.local/bin/nucleus manual
/Users/joey/.local/bin/chancery show clockwork.schedule.operate
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

   Treat it as the complete Annals product gate. Packaging coverage uses fake
   Clockwork and launchctl surfaces in an isolated home, never live bindings.
5. Treat deployment and migration as separately authorized operations.

Keep current Semantics terminology out of the preserved experiment archive, whose older
tree, path, placement, proposal, and uncertainty terms are deliberately
historical. Conversely, do not revive historical terms in current contracts.

## Compatibility and recovery

Track compatibility separately for the Annals release, library schema,
Nucleus public protocol, Clockwork definition/binding identity, immutable
tool/schema registrations, and installed packaging. Define migration and
rollback whenever persistent meaning changes.
Preserve exact domain results after later runtime failure and retain historical
decoders where Nucleus records require them.

Installation changes must respect scheduler quiescence, maintenance,
transactional database backup, release selector rollback, and forward-only
Nucleus authentication. Never edit the installed database or spool as an ad
hoc migration.

`annals/release.sh` commits, tags, and pushes; it is a publication command, not
a test command. Development completion does not authorize it or installation.

Source fixtures, experiment archives, libraries, spools, Nucleus output, and
backups can contain private source and model context. Protect each according
to the most sensitive content it can retain.

## Library interpretation changes

Named libraries share storage and graph mechanics while their stored
instructions define interpretation. Keep default broader/narrower meaning in
the initial instruction document, not unconditional shared prompt or tool text.
Capture corpus and instruction revisions at one admission boundary, bind reuse
to exact prompt/tool context, and check both revisions at material application.
Preserve committed results and unknown legacy provenance. A changed immutable
tool definition needs a new registration identity; do not rewrite historical
Nucleus schema or output records. Catalog, library schema, and provider contract
versions remain separate identities. Installation must include registered
libraries in its journaled backup, migration, and recovery scope without
implicitly adding schedules or adopting operator paths.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.
