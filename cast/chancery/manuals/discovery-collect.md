# Collect companies and jobs

Cast discovers employers and updates stored job postings through Rust code and
bounded HTTP requests. To run one collection:

```sh
cast run
cast run --force
cast run --source brave --max-requests 10
cast job refresh JOB_ID
cast job collect JOB_URL
cast --state-dir /absolute/private/state run
```

A normal run selects due work. `--force` bypasses due times but never raises the
configured budgets. `--due` explicitly selects the normal due-work behavior.
`--source` limits discovery queries by provider or query ID; careers-source
refresh remains part of that run. `--max-requests` can lower the configured HTTP
cap for one invocation. `job refresh` selects the posting source again under
ordinary source policy and the same budgets.

`automatic_excluded_ats` defaults to `["ashby"]`, including when older stored
configuration omits the field. Ordinary runs skip excluded ATS boards and do
not insert or update postings whose job or application URL identifies an
excluded ATS. Existing jobs remain readable. This policy applies to discovery,
careers collection, forced runs and `job refresh`. Source metadata can still
retain excluded board URLs. An enabled source remains subject to this policy.

`job collect` returns one normal Cast job for an exact public
posting URL. It returns an existing URL or native ATS identity without another
collection run. Otherwise it retrieves the selected posting even when its source
is disabled or its ATS is excluded from ordinary collection. It preserves an
existing source's enabled setting; a new source starts disabled. An adapter may
download a board or its pages, but only the selected posting enters Cast.
Unrelated postings and discovered careers links are not admitted by this operation.
A board URL without a posting identity is refused. The schema-one result
contains `schema_version` and `job`.

Provider requests require the applicable keys. The selected
collection step records missing credentials in its outcome. The installed wrapper loads
`THEIRSTACK_API_KEY` and `BRAVE_SEARCH_API_KEY` from the user's `.zshrc` without
printing them. The Rust executable reads these keys from its environment.

Cast searches configured TheirStack, Brave and Hacker News query families,
extracts company leads and public careers sources, then collects from supported
employer/ATS sources. It stores companies, jobs and the outcome of each selected
collection step, including failed, partial, unsupported and deferred steps.

Cast stores incoming jobs without a title substring requirement. This applies
to discovery and careers collection, including updates to existing jobs.
Configured provider queries still determine which postings discovery returns.
Consumers choose jobs by title, seniority, location or other preferences.

Supported ATS boards use separate provider/tenant company identities. Generic
JSON-LD collection requires the hiring organization's root homepage URL. The
URL must have no query or fragment. Its host must match the page or job host,
or that host's parent employer domain. A tenant path on a shared recruiting
site fails this adapter rule.
Known shared hosts such as `join.com` and `curriculo.me` require a separate
adapter. Directory results are retained as company candidates.
An exact generic job URL must produce one owned `JobPosting` whose normalized
URL matches the supplied URL. Unsupported, ambiguous and non-job pages fail
without inventing a record. Failed targeted collection retains its source,
run outcome and conservative request accounting for diagnosis.
Cast runs no model, computer-use tool, Nucleus job, CRM steward, packet builder,
email sender or application submitter.

## Budgets and progress

Default caps are 200 total and 60 daily TheirStack credits, 1,000 monthly and
30 daily Brave requests, 500 HTTP requests per run, 3,000 daily HTTP requests,
600 seconds per run and 50 careers collections per run. Targeted collection
uses the same HTTP and runtime budgets and at most 50 adapter pages by default,
controlled by `max_verifications_per_run`. These are local
controls; provider rate limits and balances are managed by each provider.
Cast must reserve a request's permitted cost before it sends the request.
If the failure outcome is ambiguous, Cast keeps the local charge.
Daily and monthly windows use UTC. The TheirStack total cap covers this
state's retained lifetime, not a monthly provider allowance.

Cast stores configuration in its state. Use the configuration commands to
inspect or replace it. Raising a cap never purchases
credits or changes a provider subscription. The run does not automatically
activate future runs. Another invocation uses stored due work and cursors.
A future scheduler would activate runs. Cast would retain collection and
budget authority.

## Stored records and run results

Cast stores company and job fields, source locators, a posting excerpt of at
most 1,200 characters, observation times and collection diagnostics. Whole API
responses and HTML documents are transient.

`first_seen_at` and `last_seen_at` record Cast observation times. Source
publication and update timestamps use separate fields. Failed or partial
fetches do not add missing observations. Third-party discovery records
`unknown`. Employer/ATS observations record
`listed`, or `unlisted` when the source supplies that flag. A posting absent
from a completed employer-source scan records `missing`; at least two complete
missing scans and 24 hours since the first missing observation record
`presumed_closed`. Rapid repeated scans keep the same 24-hour requirement.
A later employer-source observation restores `listed`.
These values are stored in each job's availability field.
Each job includes its recorded status and source collection timestamps.

A run result records each selected step's completed, failed or deferred outcome.
Its `company_observations` counts company drafts returned by discovery.
The `jobs` count in status counts
stored jobs. Read `cast export --json` and source coverage for the
actual handoff. Downstream products decide job selection and retain their own
application and notification histories.

A targeted run records `scope: "job"`, its normalized URL and source ID in the
run note. Success adds the retained job ID and `verification_pages`, the number
of adapter pages examined for that selection. Failure records its error.
Run start and finish times describe this attempt. The selected job's observation
times describe its retrieval. Targeted work preserves source scan timestamps,
status, cursors, due times and missing-job counters for other postings. Read the
run outcome and selected job to assess targeted retrieval; source health still
describes ordinary board collection.

## Recovery and privacy

Only one writer can use a selected state directory. Do not run concurrent
copies against the same state. After an interruption or failure, Cast retains
committed observations and conservative usage. Later runs can continue due work. Do not reset
SQLite counters, invent successful checkpoints, or treat a partial page as a
complete result.

Query text and fetch URLs leave the machine for their selected providers.
Extracted data, search interests and diagnostics remain private local state;
exports are the caller's responsibility. Credentials must not enter query
configuration, command arguments, database rows or logs. The local limits are
operational safeguards, not authoritative billing statements.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.
