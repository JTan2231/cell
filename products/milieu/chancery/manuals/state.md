# Milieu state and current records

Milieu owns the selected private database, accepted current records, source
metadata, and local readiness checks. Use `milieu.install.operate` for setup,
diagnosis, source enrollment, and recovery.

Milieu accepts database schema 2 only. Opening, initialization, and deployment
reject schema-one discovery state without converting or changing its records.
This release provides no database migration command. Milieu performs no
collection or runtime HTTP requests.

## Accepted current model

`milieu::current` defines `Company`, `Job`, `Source`, `Location`, `JobLocation`,
and `JobSource`. `milieu::current::SCHEMA` contains the corresponding SQLite
definitions:

```text
company      id, name
job          id, employer_id -> company, title, description, work_mode, status
source       id, name, url, operator_id -> company [optional]
location     id, name
job_location job_id -> job, location_id -> location
job_source   job_id -> job, source_id -> source, url
```

Both join tables use their two foreign keys as the composite primary key.
`employer_id` identifies who hires. `operator_id` identifies who operates a
source. Locations identify workplaces; `work_mode` is a separate opportunity field. An
opportunity can appear in several sources. Each opportunity/source pair has one URL.

The core represents one accepted current record per opportunity. Resolve duplicate
matching and conflicting values before writing that record. A write replaces
the opportunity and its workplace and source links in one transaction. The core has no
collection runs, observation history, affiliation history, assessments, or
decision history.

Milieu executes this schema when it initializes a missing database. The runtime
also retains snapshot revision, company identity aliases, and current
compatibility metadata. A `meta` table holds snapshot revision and alias
lookup values. Compatibility columns in `company`, `job`, and `source` rows carry
existing export fields, including current entity revisions, supplied dates
and evidence, compensation, and source metadata. This is not a second company
or opportunity identity model. There are no revision-history, observation,
request-ledger, or run tables.

Company and opportunity compatibility dates must be valid RFC 3339 values, with
`first_seen_at` no later than `last_seen_at`. Milieu normalizes these two fields
to UTC with nine fractional-second digits for chronological string comparison.
Updates preserve the first-seen instant and do not move the last-seen instant
backwards. An existing location ID preserves its name. There is no CLI opportunity
ingestion interface.

`milieu.discovery.explore` defines the schema-one export, public read types,
primary posting identity, and their compatibility projection. Database schema
2 and export schema 1 have separate meanings.

## State selection and access

State defaults to `~/.local/share/milieu`. Use the global `--state-dir PATH`
option or `MILIEU_STATE_DIR` to select another directory. The CLI flag can
select state independently of the environment. The installed frontend preserves
an explicit `MILIEU_STATE_DIR`.

The selected directory contains `milieu.sqlite3`. The directory uses mode 0700;
the database uses mode 0600. Initialize missing state explicitly with
`milieu init`. Other commands require supported initialized state. Initialization
creates database schema 2 and starts no collection.

Only one writer can use a state directory at a time. Mutation uses a
kernel-backed file lock. Stop callers before state maintenance or recovery.
Direct SQLite reads and writes are not supported integration surfaces.

## Local readiness and status

```sh
milieu init
milieu doctor
milieu status --json
milieu --state-dir /absolute/private/state doctor
```

State commands print readable results by default. Init prints the selected
state directory. Doctor reports usable local state. Status reports current
local record counts. Use the global `--json` flag for machine output; it is
accepted before or after the command.

Ordinary `status --json` uses response schema 3 with `snapshot_revision` and
`companies`, `jobs`, and `sources` counts. It has no provider budgets, request
usage, last-run record, or collector coverage calculations. Source metadata remains available
through record reads and exports.

Doctor JSON uses schema 2 with `database:"ready"`, `database_schema:2`, and
`state_dir`. It checks the selected local database. It does not read provider
keys or collector configuration. Local readiness establishes no current posting
availability. No external service is a readiness prerequisite.

The separate `status-snapshot --json` operational protocol retains schema 1 and
the `milieu/discovery` unit. It checks installed command availability without
opening Milieu state or checking database integrity. It does not require provider
credentials, collector configuration, or budgets. Do not parse that protocol
as ordinary `status --json`.

Operational errors exit 1 and use a text diagnostic on stderr by default. With
`--json`, stderr contains a schema-one object with `ok:false` and
`error.detail`. Invalid command syntax uses Clap's text diagnostic on stderr
and exit 2 in both modes.

## Source enrollment

```sh
milieu source add https://employer.example/careers --company-id COMPANY_ID
milieu source add https://job-boards.greenhouse.io/EMPLOYER
milieu sources list
```

`source add URL --company-id COMPANY_ID` associates an ordinary website source
with an existing company in compatibility metadata. This association does not
establish `source.operator_id` or an opportunity's employer. Without the company option,
an ordinary website URL creates or reuses a company candidate from its
hostname. For a supported ATS URL, omit `--company-id`: Milieu assigns its
canonical provider/tenant company identity and rejects an explicit override.

Source enrollment retains metadata and makes no remote request. The exported
source `enabled`, status, timestamps, cursor, and note are compatibility
metadata. No collector acts on them. Source operator and opportunity employer remain
independent accepted company references.

## Retired interfaces

Collector configuration, defaults, validation, and provider-key diagnostics
are removed. Old collector configuration can remain in metadata of existing
state; Milieu does not read or replace it. New state has no collector configuration.
Deployment accepts no `config_file` setting.

The commands `config show`, `config set`, `state reconcile-ownership`,
`source disable`, and `unresolved list` are removed. Milieu does not infer or
repair accepted employer or source-operator associations. Accepted records,
current revisions, and supplied source evidence remain available through
supported reads.

## Recovery and privacy

Installation state and Milieu data are separate recovery units. Program recovery
selects retained files and leaves the database unchanged. Select a program
that supports database schema 2. Schema-one-only programs cannot open current
state; this release cannot open schema-one state.

Schema support alone does not establish complete older CLI compatibility.
Older releases that require stored collector configuration can fail status or
diagnosis on newly initialized Milieu 0.6.0 state, even when their exports support
schema 2. Program recovery does not recreate removed configuration.

This release provides no automatic database migration, pruning, destructive
reset, or state uninstaller. Stop if recovery requires an incompatible
program/state pair or unsupported row edits. Do not repair individual SQLite
rows by hand.

State, exports, and diagnostic captures can contain private source locators,
posting text, supplied evidence, and dates. Keep them within the selected
user's data boundary. Milieu reads no provider credentials and performs no
remote collection.

## Compatibility and limits

Database schema, export schema, feature contract version, and product release
are distinct. State contract 3 retires schema-one runtime support and the
local collector controls. Ordinary status response schema 3 is separate from
the preserved operational schema-one probe and discovery export schema 1.
The public Snapshot and record-read handoff remain at read contract 3.

No future database migration, retention horizon, state-size bound, or
deprecation window is promised. State operations create no schedule, agent
job, CRM case, packet, application, or email. Source availability remains
outside Milieu's authority.
