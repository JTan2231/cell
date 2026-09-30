# Clew

Clew is a private ledger notepad. It retains supplied notes and optional status
in an append-only history. Entries can stand alone, belong to a named thread,
and link to stable identities outside Clew. It also maintains an explicit
application-report view for Cast jobs and an optional daily application email.

Read `chancery show ID` for one feature or procedure. Read
`chancery resolve ID` for its required contracts and compatibility gaps. These
commands read the installed publication. They do not check live readiness or
authorize a note, report, send, schedule, or installation.

## Features

| ID | Read this to understand |
| --- | --- |
| `clew.ledger.use` | Supplied notes, optional status, named threads, optional external links, local search, entry reads, exact retries, corrections, and retractions. |
| `clew.application.track` | Explicit Cast job reports, retained-job search, supplied application status, job history, and the boundary between application reports and plain job links. |
| `clew.digest.email` | Application selection, copied notes, missing job context, frozen occurrences, send authority, uncertain submission, and scheduling failures. |
| `clew.state` | Private state, initialization and integrity, program selection, email maintenance, older-ledger migration, backup, and compatible recovery. |

Use `clew.install.operate` for installation, verification, guarded migration,
schedule preparation, backup, and recovery procedures. Its required feature
contracts supply the detailed behavior.

## How the features work together

An authorized caller appends supplied text under a stable write ID. Optional
`--thread NAME` selects or creates a named thread in the same transaction. A
thread holds only an ID and name. Optional external references identify objects
outside Clew; they do not create local subjects or define thread membership.
A correction appends a replacement or retraction and preserves original text.

Generic search reads retained ledger text and identifiers. The conversational
agent can retrieve a matching thread and answer from its statements and
correction history. Clew does not interpret the question, verify completion,
execute migrations, or run a model.

An explicit `--cast-job JOB_ID` report selects the application view. A plain
`--ref cast.job JOB_ID` link supplies context and does not change application
status or email. The daily email reads only active application reports and joins
a separate retained Cast snapshot. Missing context does not omit qualifying
applications. A send freezes its message and identity before invoking Email.
Provider acceptance and final inbox delivery are separate outcomes.

Clew owns entries, thread membership, reference associations, retry identity,
ledger state, email selection, frozen occurrences, and its schedule definition.
The caller supplies statements and status. Cast owns job identities and details.
Platter supplies legacy migration mappings and owns prepared material. Email
owns submission; Clockwork owns activation and failure halts.

Private state stays outside source and release trees. Installation publishes the
matching contracts with the selected program. Guarded migration converts older
job history without inventing threads or changing frozen email. Recovery
preserves history and requires compatible program and state versions.

Required dependencies define documentation compatibility and complete reading.
Related references provide navigation. Neither transfers product authority or
establishes live dependency readiness.
