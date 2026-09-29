# Clew

Clew retains the application status and notes that the user reports for jobs
owned by Cast. It preserves every report in an append-only ledger. It can also
render and send a deterministic daily email of the tracked applications except
current rejections.

Read `chancery show ID` for one feature or procedure. Read
`chancery resolve ID` for its required contracts and compatibility gaps. These
commands read the installed publication. They do not check live readiness or
authorize a report, send, schedule, or installation.

## Features

| ID | Read this to understand |
| --- | --- |
| `clew.application.track` | Retained-job search, user-supplied status and notes, exact write identity, current records, history, corrections, and retractions. |
| `clew.digest.email` | Daily selection, copied notes, missing job context, message occurrences, send authority, uncertain submission, and scheduled failure behavior. |
| `clew.state` | Private state, initialization and integrity checks, immutable program and provider selection, email maintenance, ledger migration, backup, and recovery guarantees. |

Use `clew.install.operate` for installation, verification, guarded migration,
schedule preparation, backup, and recovery procedures. Its required feature
contracts supply the detailed behavior. The existing operation ID and contract
version remain supported.

## How the features work together

An authorized agent finds the intended retained Cast job and resolves material
ambiguity with the user. Clew accepts the exact job identity and the user's
report. A stable write ID permits an unchanged retry. A correction appends a
replacement or retraction; it does not rewrite history.

The daily email reads current ledger records and joins a separate retained Cast
snapshot. It keeps qualifying applications visible when job context is missing.
A send freezes its message and identity before invoking Email. Provider
acceptance and final inbox delivery remain separate outcomes.

Clew owns reports, ledger state, email selection, retained occurrences, and its
schedule definition. The user owns every supplied status and send authority.
Cast owns job identities and details. Platter supplies legacy migration mappings
and owns prepared material. Email owns submission; Clockwork owns activation
and failure halts. Clew runs no model and does not apply to jobs or contact
employers.

Private state stays outside source and release trees. Installation publishes the
matching feature pages with the selected program. Recovery preserves ledger and
email history and must use compatible program and state versions.

Required dependencies define documentation compatibility and complete reading.
Related references provide navigation. Neither transfers product authority or
establishes live dependency readiness.
