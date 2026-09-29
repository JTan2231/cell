# Cast state and collection policy

Cast owns the selected discovery database, complete collection configuration,
source enrollment, local budget counters, and the supported employer-ownership
repair. These are local current-user interfaces. Use `cast.install.operate`
for the setup, configuration, diagnosis, and recovery procedure.

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

`config show` returns the complete selected configuration. `config set --file`
validates and replaces that complete configuration. Inspect the current values
before editing them so that the replacement preserves intended queries,
intervals, adapter parameters, and caps. Changes affect later collection; they
do not reset consumed allowance, purchase credits, or change provider billing.

The default configuration includes TheirStack, Brave, and Hacker News queries.
Configuration schema 1 contains query families, budget settings, careers
intervals, adapter-page limits, and `automatic_excluded_ats`. Query terms and
parameters determine what discovery requests. A complete stored configuration
is the supported input; credentials do not belong in it.

`automatic_excluded_ats` is an array of distinct supported ATS names: `ashby`,
`greenhouse`, or `lever`. It defaults to `["ashby"]`, including when existing
configuration omits the field. An explicit empty array removes these ATS
exclusions. Each source still has its own enabled setting. Read
`cast.discovery.collect` for how ordinary and exact-job collection apply this
policy.

`source add URL --company-id COMPANY_ID` associates an ordinary website source
with an existing company. Without the company option, an ordinary website URL
creates or reuses a company candidate from its hostname. For a supported ATS
URL, omit `--company-id`: Cast assigns its canonical provider/tenant company
identity and rejects an explicit company override.

`source disable SOURCE_ID` removes the source from ordinary collection while
retaining the source, jobs, and collected data. It does not block explicit
`job collect` requests. Source enrollment and collection are separate actions.

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

These are conservative local admission controls. They do not measure a
provider's available balance or establish its billing statement. The TheirStack
total cap is not a monthly provider allowance. Raising a cap does not obtain
additional provider credits. Read `cast.discovery.collect` for request
reservation, ambiguous-failure accounting, and run limits.

Configuration and diagnostic reads describe selected local state. `doctor`
can check configuration and credential presence. Provider authentication,
available balance, and actual collection require provider interactions. Local
readiness or an installed Chancery entry is not proof of those conditions.

## Employer-ownership reconciliation

```sh
cast state reconcile-ownership
```

This explicit local repair applies the current ownership rules to older stored
records. It holds the mutation lock and commits one transaction. It assigns
ATS sources and their jobs to the provider/tenant company, sets older JSON-LD
jobs to `unknown`, marks their sources for the next collection, and restores
affected search-candidate names to their domains. It applies current adapter
rules to shared recruiting hosts and clears their company domains, website
URLs, and identity aliases.

The JSON result reports `moved_sources`, `moved_jobs`, `quarantined_jobs`,
`renamed_candidates`, and `cleared_shared_identities`. These counts describe
the selected repair, not external coverage. Source and job IDs, paid request
accounting, run history, query coverage, and cursors remain intact. Changed jobs
and companies gain revisions. A later collection uses the corrected
associations.

The repair uses stored records and makes no provider requests. Repeating it
leaves material records unchanged, while each invocation advances the snapshot
revision. Read `cast.discovery.explore` for the meaning of retained identities,
attribution, availability, and revisions.

## Recovery and privacy

Before state recovery, stop all callers using the selected directory and make
a private consistent SQLite backup, including live sidecars when relevant.
Installation state and discovery state are different recovery units. Restoring
an older program alone cannot restore newer domain state or configuration.

This release provides no automatic database migration, pruning, destructive
reset, or state uninstaller. Use the supported state and configuration
commands. Do not erase state to clear a budget or repair individual rows by
hand. Preserve successful observations and conservative request usage after
an interrupted or partial collection.

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
