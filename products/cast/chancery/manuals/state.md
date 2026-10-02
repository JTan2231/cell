# Cast state and current model definitions

Cast owns the selected discovery database, retained collection configuration,
source metadata, local budget counters, and the supported employer-ownership
repair. These are local current-user interfaces. Use `cast.install.operate`
for the setup, configuration, diagnosis, and recovery procedure.

Cast performs no collection. Configuration, source controls, budget reads,
and ownership reconciliation remain available for existing state. These
interfaces do not populate the unused current model or refresh retained jobs.

## Unused current model

`cast::current` defines `Company`, `Job`, `Source`, `Location`, `JobLocation`,
and `JobSource`. `cast::current::SCHEMA` contains the corresponding SQLite
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
source. Locations identify workplaces; `work_mode` is a separate job field.
A job can appear in several sources. Each job/source pair has one URL.

The model represents one accepted current record per job. It contains no
collection runs, observations, timestamps, evidence, affiliation history, or
assessments. Duplicate matching and conflicting values require a decision
before that record changes; the model does not preserve that decision history.

These are unused library definitions. Cast does not execute this SQL, select
this model, or migrate data into it. The CLI, installer setup, `Store`, and
downstream reads retain the existing discovery state and output. Data
migration and activation are separate work.

## State selection and access

State defaults to `~/.local/share/cast`. Use the global `--state-dir PATH`
option or `CAST_STATE_DIR` to select another directory. The CLI flag can select
state independently of the environment. The installed wrapper preserves an
explicit `CAST_STATE_DIR`.

The selected directory contains `cast.sqlite3`. The directory uses mode 0700;
the database uses mode 0600. Configuration is stored in database metadata.
Initialize missing state explicitly with `cast init`. Initialization preserves
existing discovery records and consumed budgets and starts no collection.
Other commands require supported initialized state.

Only one writer can use a state directory at a time. Mutation uses a
kernel-backed file lock. Stop callers before state maintenance or recovery.
Direct SQLite reads and writes are not supported integration surfaces.

## Configuration and source interfaces

```sh
cast init
cast doctor
cast config show
cast config set --file /absolute/path/to/config.json
cast source add https://employer.example/careers --company-id COMPANY_ID
cast source add https://job-boards.greenhouse.io/EMPLOYER
cast source disable SOURCE_ID
cast --state-dir /absolute/private/state config show
```

State commands print readable results by default. Init prints the selected
state directory. Doctor and config show label configuration and budget units.
Configuration and source writes print short receipts. Ownership repair labels
its result counts. Use the global `--json` flag for existing machine success
results. It is accepted before or after any state command.

Operational errors exit 1 and use a text diagnostic on stderr by default.
With `--json`, stderr contains a schema-one object with `ok:false` and
`error.detail`. Invalid command syntax uses Clap's text diagnostic on stderr
and exit 2 in both modes.

`config show --json` returns the complete selected machine configuration. `config set --file`
validates and replaces that complete configuration. Inspect the current values
before editing them so that the replacement preserves retained queries,
intervals, adapter parameters, and caps. Changes update stored configuration;
they do not start collection, reset consumed allowance, purchase credits, or
change provider billing.

The default configuration includes TheirStack, Brave, and Hacker News queries.
Configuration schema 1 contains query families, budget settings, careers
intervals, adapter-page limits, and `automatic_excluded_ats`. These values
remain accepted configuration fields. A complete stored configuration is the
supported input; credentials do not belong in it.

`automatic_excluded_ats` is an array of distinct supported ATS names: `ashby`,
`greenhouse`, or `lever`. It defaults to `["ashby"]`, including when existing
configuration omits the field. An explicit empty array removes these ATS
exclusions in the retained configuration. Each source still has its own enabled
setting. No collection command applies these settings in this release.

`source add URL --company-id COMPANY_ID` associates an ordinary website source
with an existing company. Without the company option, an ordinary website URL
creates or reuses a company candidate from its hostname. For a supported ATS
URL, omit `--company-id`: Cast assigns its canonical provider/tenant company
identity and rejects an explicit company override.

`source disable SOURCE_ID` clears the retained enabled setting while preserving
the source, jobs, and collected data. Source add and disable make no remote
request.

## Local budget defaults and units

| Configuration limit | Default | Scope and unit |
| --- | --- | --- |
| TheirStack total | 200 | Credits across this state's retained lifetime. |
| TheirStack daily | 60 | Credits per UTC day. |
| Brave monthly | 1,000 | Requests per UTC month. |
| Brave daily | 30 | Requests per UTC day. |
| HTTP per run | 500 | Requests for one collection invocation. |
| HTTP daily | 3,000 | Requests per UTC day. |
| Runtime | 600 | Seconds for one run. |
| Careers collections | 50 | Source collections for one run. |
| `max_verifications_per_run` | 50 | Adapter pages for targeted collection. |

These are retained configuration defaults and historical local accounting
units. No collector consumes allowance in this release. They do not measure a
provider's available balance or establish its billing statement. The TheirStack
total cap is not a monthly provider allowance. Raising a cap does not obtain
additional provider credits.

Configuration and diagnostic reads describe selected local state. `doctor`
can check configuration and credential presence. It does not check provider
authentication or balance. Local readiness or an installed Chancery entry
does not establish those conditions or current job availability.

## Employer-ownership reconciliation

```sh
cast state reconcile-ownership
```

This explicit local repair applies the current ownership rules to older stored
records. It holds the mutation lock and commits one transaction. It assigns
ATS sources and their jobs to the provider/tenant company, sets older JSON-LD
jobs to `unknown`, resets their retained source collection status, and restores
affected search-candidate names to their domains. It applies current adapter
rules to shared recruiting hosts and clears their company domains, website
URLs, and identity aliases.

The result reports `moved_sources`, `moved_jobs`, `quarantined_jobs`,
`renamed_candidates`, and `cleared_shared_identities`. These counts describe
the selected repair, not external coverage. Source and job IDs, paid request
accounting, run history, query coverage, and cursors remain intact. Changed jobs
and companies gain revisions. These associations change in retained reads;
reconciliation does not retrieve source facts.

The repair uses stored records and makes no provider requests. Repeating it
leaves material records unchanged, while each invocation advances the snapshot
revision. Read `cast.discovery.explore` for the meaning of retained identities,
attribution, availability, and revisions.

## Recovery and privacy

Installation state and discovery state are different recovery units. Restoring
an older program alone cannot restore newer domain state or configuration.

This release provides no automatic database migration, pruning, destructive
reset, or state uninstaller. Use the supported state and configuration
commands. Do not erase state to clear a budget or repair individual rows by
hand. Preserve retained observations and conservative request usage.

State, exports, and diagnostic captures can contain private search interests,
source locators, posting excerpts, and collection history. Keep credentials in
the caller environment and user-owned shell configuration. The Rust payload
reads `THEIRSTACK_API_KEY` and `BRAVE_SEARCH_API_KEY` from its environment;
`cast.installation` owns the installed wrapper's loading and privacy contract.

## Compatibility and limits

Database schema, configuration schema, export schema, feature contract version,
and product release are distinct. Configuration schema remains 1 with
`automatic_excluded_ats`; older programs that reject unknown fields cannot
read configuration written with that field. Program recovery does not remove
it or restore previous state.

No future database migration, retention horizon, state-size bound, or
deprecation window is promised. State operations create no schedule, agent
job, CRM case, packet, application, or email. External billing and source
availability remain outside Cast's authority.
