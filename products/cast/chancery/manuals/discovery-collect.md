# Cast company and job collection

Cast discovers employer candidates and updates stored postings through Rust
code and bounded HTTP requests. It owns source admission, extracted
observations, identity correlation, collection outcomes, and conservative
request accounting. External sources supply posting facts and manage their
own availability and billing.

Collection requires supported initialized state, valid configuration, network
access, and the applicable provider keys. Read `cast.state` for configuration,
source controls, local budget defaults, and state recovery. Read
`cast.discovery.explore` for the retained record meanings and read handoff.

## Ordinary collection interfaces

```sh
cast run
cast run --due
cast run --force
cast run --source brave --max-requests 10
cast job refresh JOB_ID
cast --state-dir /absolute/private/state run
```

A normal run selects due work. `--due` explicitly selects that behavior.
`--force` bypasses due times and does not raise budgets. `--source` limits
discovery queries by provider or query ID; careers-source refresh remains part
of that run. `--max-requests` can lower the configured HTTP cap for one
invocation. `job refresh` selects the posting source again under ordinary
source policy and the same budgets.

Cast searches configured TheirStack, Brave, and Hacker News query families,
extracts company leads and public careers sources, and collects supported
employer/ATS sources. Configured query terms determine what discovery returns.
Cast stores incoming jobs without a title substring requirement, including
updates to existing jobs. Consumers choose jobs by title, seniority, location,
or other preferences.

Ordinary collection skips boards excluded by `automatic_excluded_ats` and does
not insert or update postings whose job or application URL identifies an
excluded ATS. This applies to discovery, careers collection, forced runs, and
`job refresh`. An enabled source remains subject to that policy. Existing jobs
remain readable, and source metadata can retain excluded board URLs.

## Exact job-URL collection

```sh
cast job collect JOB_URL
```

`job collect` returns one normal Cast job for an exact public posting URL.
It first resolves an exact retained native ATS identity or normalized URL and
returns that job without another collection run. Otherwise it retrieves the
selected posting even when its source is disabled or its ATS is excluded from
ordinary collection. It preserves an existing source's enabled setting; a
new source starts disabled.

A Greenhouse posting URL uses the single-job endpoint and requires a matching
returned posting ID. A failed request does not fall back to a board download.
Other ATS adapters may download a board or its pages, but only the selected
posting enters Cast. Unrelated postings and discovered careers links are not
admitted by this operation. A board URL without a posting identity is refused.
The default text receipt identifies the selected job and URL. With the global
`--json` flag, the existing schema-one result contains `schema_version` and
`job`. Machine callers must pass that flag explicitly.

An exact generic job URL must produce one owned `JobPosting` whose normalized
URL matches the supplied URL. Unsupported, ambiguous, absent, and non-job
pages fail without inventing a record. Failed targeted collection retains its
source, run outcome, and conservative request accounting for diagnosis.

## Command output

Collection and refresh print readable run outcomes and local status by default.
Use the global `--json` flag for their existing machine results. The flag is
accepted before or after the command, including `cast job collect URL --json`.
Operational errors exit 1 and use a text diagnostic on stderr by default.
With `--json`, stderr contains a schema-one object with `ok:false` and
`error.detail`. Invalid command syntax uses Clap's text diagnostic on stderr
and exit 2 in both modes.

## Source ownership and support

Supported ATS boards use separate canonical provider/tenant company
identities. Website-domain and ATS-tenant records can describe the same real
employer; a shared name alone does not merge them.

Generic JSON-LD collection requires the hiring organization's root homepage
URL, with no query or fragment. Its host must match the page or job host, or
that host's parent employer domain. A tenant path on a shared recruiting site
fails this rule. Known shared hosts such as `join.com` and `curriculo.me`
require a separate adapter. Directory results remain company candidates.

The adapter records `employer_ats` or `employer_jsonld_owned` attribution only
under the corresponding ownership rules. Source-native IDs remain scoped to
their source namespace. Cast does not substitute a model, computer-use tool,
Nucleus job, or CRM steward when a source is unsupported.

## Request admission and progress

Each request must fit the selected state's local caps. Cast reserves the
permitted request cost before sending it. An ambiguous failure keeps its local
charge. Forced and targeted collection still use ordinary request and runtime
budgets. Targeted collection also obeys `max_verifications_per_run`, which
limits adapter pages. Read `cast.state` for the defaults, units, and UTC
accounting windows.

The Rust executable reads `THEIRSTACK_API_KEY` and `BRAVE_SEARCH_API_KEY` from
its environment. A selected step records missing credentials in its outcome.
`cast.installation` owns the installed frontend's credential-loading contract.

A run activates no future run. Another invocation uses stored due work and
cursors. Cast installs no schedule and does not purchase provider allowance.
Provider rate limits can restrict work even when a local cap permits it.

## Outcomes and observation scope

A run retains companies, jobs, source metadata, and each selected collection
step's outcome, including completed, partial, failed, unsupported, and deferred
work. Its `company_observations` counts company drafts returned by discovery.
The `jobs` count in status counts retained jobs. Neither count establishes
complete coverage of external postings. Read source and query coverage with
the export to assess the selected work.

Whole API responses and HTML documents are transient. Cast retains extracted
fields, source locators, observation times, collection diagnostics, and posting
excerpts of at most 1,200 characters. `first_seen_at` and `last_seen_at` describe
Cast observations; source publication and update dates remain separate.

Failed or partial scans do not add missing observations. Employer-source
availability changes require the completed scans described by
`cast.discovery.explore`. A partial result is not proof that unobserved
postings are closed or absent.

A targeted run records `scope: "job"`, its normalized URL, and its source ID in
the run note. Success adds the retained job ID and `verification_pages`, the
number of adapter pages examined. Failure records its error. Run start and
finish times describe that attempt; selected-job observation times describe
its retrieval.

Targeted work preserves source scan timestamps, status, cursors, due times,
and other postings' missing-job counters. Read the selected job and targeted
run outcome to assess its result. Source health still describes ordinary board
collection.

## Failure, recovery, and privacy

Only one writer can use the selected state directory. Do not run concurrent
writers. An interrupted or failed collection preserves committed observations
and conservative usage. Later runs can continue due work under stored state
and budgets. Inspect run, source-health, and budget diagnostics before another
invocation. Do not reset counters, invent successful checkpoints, or treat a
partial page as complete collection.

Query text and fetch URLs leave the machine for their selected providers.
Extracted evidence, search interests, and diagnostics remain private local
state. The caller owns any export or external diagnostic capture. Credentials
must not enter command arguments, query configuration, database rows, or logs.

Collection creates no personal recommendation, application packet, CRM case,
application submission, or email. Downstream products retain authority for
those results.

## Compatibility and limits

Collection contract 6 preserves the distinction between exact-job observation
and ordinary collection. The configuration, database, and export schema
numbers remain separate. Older programs can reject configuration containing
`automatic_excluded_ats`; see `cast.state` before program recovery.

No provider-availability, completion-latency, future API-compatibility service
level, migration promise, or deprecation window is defined. TheirStack, Brave,
Hacker News, employer websites, and public ATS APIs have no dedicated installed
Chancery contracts. Resolution retains those external-reliance gaps. An
installed contract does not prove current provider readiness.
