# Collect companies and jobs

Cast discovers employers and updates stored job postings through Rust code and
bounded HTTP requests. To run one collection:

```sh
cast run
cast run --force
cast run --source brave --max-requests 10
cast job refresh JOB_ID
cast --state-dir /absolute/private/state run
```

A normal run selects due work. `--force` bypasses due times but never raises the
configured budgets. `--due` explicitly selects the normal due-work behavior.
`--source` limits discovery queries by provider or query ID; careers-source
refresh remains part of that run. `--max-requests` can lower the configured HTTP
cap for one invocation. `job refresh` fetches the posting source again within
the same budgets. Provider requests require the applicable keys. The selected
collection step records missing credentials in its outcome. The installed wrapper loads
`THEIRSTACK_API_KEY` and `BRAVE_SEARCH_API_KEY` from the user's `.zshrc` without
printing them. The Rust executable reads these keys from its environment.

Cast searches configured TheirStack, Brave and Hacker News query families,
extracts company leads and public careers sources, then collects from supported
employer/ATS sources. It stores companies, jobs and the outcome of each selected
collection step, including failed, partial, unsupported and deferred steps.
Supported ATS boards use separate provider/tenant company identities. Generic
JSON-LD collection requires the hiring organization's root homepage URL. The
URL must have no query or fragment. Its host must match the page or job host,
or that host's parent employer domain. A tenant path on a shared recruiting
site fails this adapter rule.
Known shared hosts such as `join.com` and `curriculo.me` require a separate
adapter. Directory results are retained as company candidates.
Cast runs no model, computer-use tool, Nucleus job, CRM steward, packet builder,
email sender or application submitter.

## Budgets and progress

Default caps are 200 total and 60 daily TheirStack credits, 1,000 monthly and
30 daily Brave requests, 500 HTTP requests per run, 3,000 daily HTTP requests,
600 seconds per run and 50 careers collections per run. These are local
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

A run result records the rows stored and each selected step's completed,
failed or deferred outcome. Read `cast export --json` and source coverage for the
actual handoff. Downstream products decide job selection and retain their own
application and notification histories.

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
