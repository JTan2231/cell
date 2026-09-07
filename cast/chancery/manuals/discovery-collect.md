# Collect companies and jobs

Cast discovers employers and refreshes observed job postings with ordinary Rust
code and bounded HTTP requests. Run one ad hoc collection with:

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
cap for one invocation. `job refresh` selects the posting source for another bounded fetch, with the
same budgets. Provider requests require the appropriate keys; missing
credentials are recorded in the selected collection step's outcome. The installed wrapper loads
`THEIRSTACK_API_KEY` and `BRAVE_SEARCH_API_KEY` from the user's `.zshrc` without
printing them. The Rust executable reads these keys from its environment.

Cast searches configured TheirStack, Brave and Hacker News query families,
extracts company leads and public careers sources, then collects from supported
employer/ATS sources. It stores companies, jobs and the outcome of each selected
collection step, including failed, partial, unsupported and deferred steps.
Supported ATS boards use separate provider/tenant company identities. Generic
JSON-LD collection requires a hiring-organization root homepage URL with no
query or fragment whose host matches the page/job host or its parent employer
domain. A tenant path on a shared recruiting site fails this adapter rule.
Known shared hosts such as `join.com` and `curriculo.me` require a separate
adapter. Directory results are retained as company candidates.
Cast runs no model, computer-use tool, Nucleus job, CRM steward, packet builder,
email sender or application submitter.

## Budgets and progress

Default caps are 200 total and 60 daily TheirStack credits, 1,000 monthly and
30 daily Brave requests, 500 HTTP requests per run, 3,000 daily HTTP requests,
600 seconds per run and 50 careers collections per run. These are local
controls; provider rate limits and balances are managed by each provider.
A request must reserve its permitted cost before sending. Ambiguous failures
remain conservatively charged locally rather than silently restoring capacity.
Daily and monthly windows use UTC. The TheirStack total cap covers this
state's retained lifetime, not a monthly provider allowance.

Configuration is retained in Cast state and can be inspected and replaced
through the supported configuration commands. Raising a cap never purchases
credits or changes a provider subscription. The run does not automatically
activate future runs. Another invocation considers retained due work and
cursors; a future scheduler would own activation, while Cast retains its own
collection and budget authority.

## Evidence and success

Cast stores company and job fields, source locators, a posting excerpt of at
most 1,200 characters, observation times and collection diagnostics. Whole API
responses and HTML documents are transient.

`first_seen_at` and `last_seen_at` record Cast observation times. Source
publication and update timestamps are stored in separate fields. Failed or partial fetches do not add missing observations. Third-party discovery records `unknown`. Employer/ATS observations record
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

One writer owns a selected state directory. Avoid concurrent copies aimed at
the same state. Interrupted or failed collection retains committed observations
and conservative usage; later invocations can continue due work. Do not reset
SQLite counters, invent successful checkpoints, or treat a partial page as a
complete result.

Query text and fetch URLs leave the machine for their selected providers.
Extracted data, search interests and diagnostics remain private local state;
exports are the caller's responsibility. Credentials must not enter query
configuration, command arguments, database rows or logs. The local limits are
operational safeguards, not authoritative billing statements.
