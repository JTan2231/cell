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
cap for one invocation. `job refresh` selects the posting source for a bounded
fresh check, with the same budgets. Provider requests require the appropriate
keys; missing credentials leave explicit coverage gaps. The installed wrapper loads
`THEIRSTACK_API_KEY` and `BRAVE_SEARCH_API_KEY` from the user's `.zshrc` without
printing them. The Rust executable reads these keys from its environment.

Cast searches configured TheirStack, Brave and Hacker News query families,
extracts company leads and public careers sources, then refreshes supported
employer/ATS sources. A discovered provider posting is an observation; its
presence is not proof of personal fit or independent employer verification.
Unsupported, failed, incomplete and deferred work remains distinguishable.
Supported ATS boards own separate provider/tenant company identities; a linking
directory or aggregator does not own the board's postings. Generic JSON-LD needs
a hiring-organization root homepage URL with no query or fragment; its host must
match the page/job host or its parent employer domain. A tenant path on a shared
recruiting site does not satisfy this rule. Known shared hosts such as `join.com`
and `curriculo.me` require a separate adapter. Unproven directory results remain
candidates.
Cast runs no model, computer-use tool, Nucleus job, CRM steward, packet builder,
email sender or application submitter.

## Budgets and progress

Default caps are 200 total and 60 daily TheirStack credits, 1,000 monthly and
30 daily Brave requests, 500 HTTP requests per run, 3,000 daily HTTP requests,
600 seconds per run and 50 careers verifications per run. These are
local controls; provider rate limits and actual balances remain separate.
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

Cast retains distilled company and posting fields, source locators, a posting
excerpt of at most 1,200 characters, observation times, coverage and run
diagnostics. Whole API responses and HTML documents are transient. A retained digest or source URL
cannot reconstruct discarded source bytes. There is no promise that external
sources are exhaustive or remain unchanged after collection.

`first_seen_at` and `last_seen_at` are Cast observations. Source publication and
update timestamps retain their separate attribution; they are not Cast
verification times. Source failure and incomplete pagination never by themselves
mean that a job closed. Third-party discovery starts with `unknown` availability;
`listed` requires an employer/ATS observation. An explicit employer/ATS unlisted
flag records `unlisted`. Only completed authoritative collection contributes to
Cast's absence policy: an absent posting becomes `missing`, and becomes
`presumed_closed` after at least two complete missing scans and at least 24 hours
since the first missing observation. Rapid repeated scans cannot accelerate that
time requirement. A later employer observation can restore `listed`.
These states do not establish a hiring or application outcome.
Inspect each source's status and successful check time before treating a posting
as currently available.

A run result proves what Cast retained and which work settled, failed or was
deferred. It does not prove that every provider succeeded or the entire job
market was searched. Read `cast export --json` and source coverage for the
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
