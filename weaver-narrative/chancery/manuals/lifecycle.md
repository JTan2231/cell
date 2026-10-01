# Configuration, readiness, and maintenance

Weaver owns its reading configuration, private state, runner exclusion, owned
admission holds, and matched program/provider release selection. Use this
feature to interpret those boundaries. Use `chancery show weaver.install.operate`
for the procedure and `chancery show weaver.narrative.write` for authoring and
exact assignment recovery.

## Interfaces and configuration

```sh
weaver config
weaver init --annals-config /absolute/decisions/config.toml
weaver init --annals-config /absolute/decisions/config.toml --annals-binary /absolute/annals
weaver doctor
weaver status-snapshot --json
weaver maintenance status
weaver maintenance hold RUN_ID
weaver maintenance drain
weaver maintenance release RUN_ID
weaver-install inspect
```

The explicit Annals config must select an existing identity-bound decisions
library. Weaver does not create that library, operate its source intake, or own
its accepted documents. Initialization can also select an absolute Annals
executable. Coordinated installation selects the current user's
`~/.local/bin/annals`.

Init creates schema 1 only in an absent or empty database and atomically saves
reading configuration. Existing documents remain unchanged. Unsupported state
is refused. Reconfiguration takes the same runner lock as authoring.

One runner lock excludes other write, write-many, resume, and initialization
operations. A batch handles independent jobs concurrently inside that lock.
Configuration is a reader selection, not a source inventory or feed checkpoint.
Resume uses current reading configuration while preserving its exact request.

## Readiness and status

Doctor reads database integrity and source readiness and checks required Nucleus
capabilities. Source inspection reads the Annals start cursor without changing
the library. These checks create no domain record, model job, or narrative.
Annals owns its source identity and reading contract. Nucleus owns execution,
authentication, and its live job inventory.

Doctor uses ordinary admission when Weaver has no maintenance hold, even if a
caller supplies `CELL_DEPLOYMENT_RUN_ID`. When Weaver is held, doctor requires
that ID to match its sole hold. It keeps the ID for Nucleus deployment readiness.

`weaver status-snapshot --json` declares `weaver/author` as on demand. Its
incomplete snapshot does not claim live readiness. Doctor and maintenance
supply that separate evidence. Documentation presence, a selected executable,
or an incomplete observation cannot prove live readiness.

Doctor and maintenance describe selected state and available dependency
evidence at invocation. Installation inspection describes selected release metadata.
There is no promised readiness horizon, response latency, service availability,
or release cadence.

## Owned maintenance

Maintenance returns `maintenance.protocol_version=1`, `holds`, `drained`, and
`nonterminal_jobs`. A run ID owns its hold. A hold blocks new writes, batches,
and revisions. Resume can settle an existing exact assignment. Hold and release
preserve other owners' holds.

Drain requires no admitted Weaver process and no nonterminal Weaver Nucleus job.
An unavailable job inventory remains unknown and cannot prove drain. An empty
process list alone cannot establish settlement. Use exact authoring recovery
under `weaver.narrative.write`; do not invent a result, retry failed work, delete
a hold, or edit SQLite to make maintenance proceed.

Weaver defines no background worker, service, or Clockwork binding. Nucleus
owns its separate service and credentials. Holding Weaver grants no authority
to operate either Annals or Nucleus outside its own contract.

## Release and installation guarantees

Weaver is an independent release. Its CLI, installer, provider, and private
application directory are named Weaver. The Cell source and active Semantics
project are `weaver-narrative`. The predecessor `weaver` project is permanently
retired; its workflow records are not imported or replaced.

The maintained installer uses the shared Cell immutable file transaction
and adapter. Public binary and Chancery selectors follow the selected release.
It refuses foreign selectors and unsupported legacy installation formats. Direct installer `install` and `recover` are refused;
use the Cell coordinator. Read-only inspection remains available. Uninstall detaches owned selectors and retains private state.

The first installation requires a private settings file containing only
`weaver.annals_config`, an absolute path to the existing decisions config.
Later deployments reuse the stored path unless settings select another path.
Compatible Annals and Nucleus releases are installation dependencies. Bazaar
prompt contents must already exist before new authoring preparation; deployment
does not supply missing prompt text.

The coordinator holds and drains Weaver, selects the immutable program/provider
release, and initializes or configures state through the product initializer.
It performs no separate artifact-integrity, state-integrity, source-read, or
Nucleus-readiness checks. Ordinary product checks remain unchanged. It releases only its own hold. Installation
publishes program bytes locally. It does not author or publish a narrative,
send email, retry a job, or guarantee future model availability.

An interrupted deployment uses the coordinator's retained transaction recovery.
Before candidate publication, the prior installation remains selected. After
publication, Weaver can finish forward with the recorded reading configuration
and supported database. Unproved recovery retains the named hold. The Cell
coordinator owns this cross-product procedure; no dedicated installed Chancery
contract covers it. This remains a resolver gap.

## State and privacy

| Item | Current-user location |
| --- | --- |
| Private state | `~/Library/Application Support/Weaver` |
| Document and recovery store | `weaver.sqlite` under private state |
| Annals reader selection | `config.json` under private state |

The database has one `documents` table containing saved output, exact
invocation, an optional pending reply, and execution metadata. This describes
retained state; it is not a supported direct database integration surface.
Use the CLI and provider-owned Rust client for documents.

Private files use mode 0600 and state directories use mode 0700. Keep database,
and configuration private. A pending reply can contain a source page.
Nucleus may retain the full direction, source reads, and document. Keep
credential recovery under Nucleus authority.

Schema 1 has no predecessor migration. There is no automatic pruning, retention
horizon, database-capacity promise, future compatibility lifetime, or deprecation
interval. Missing or unsupported state stops work; it does not create a
replacement database. State deletion requires separate authority.

## Command observation

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors preserve
command results. A usage row does not establish readiness or domain success.

## Related contracts

- Read `chancery show weaver.narrative.write`.
- Read `chancery show weaver.install.operate`.
- Read `chancery show weaver.develop.change`.
- Read `chancery show annals.decision-account.exchange`.
- Read `chancery show nucleus.service`.
