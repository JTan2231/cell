# Read prepared opportunities

Use this interface to find the retained opportunity behind a prepared Platter
packet. Platter owns the reference and packet facts. A consumer owns its
application history and status.

```sh
platter opportunities list
platter opportunities list --query 'company or role'
platter opportunities list --query 'https://jobs.ashbyhq.com/company/posting'
platter opportunities show REFERENCE
```

Both commands return schema-one JSON with `schema_version` and `items`. List
returns every matching opportunity. Show requires an exact reference and returns
one item, or fails if it is absent. `--json` is accepted but is not required.
Rust callers use `platter::api::Client`, `OpportunityList` and `Opportunity`.

Each item contains `reference`, `cast_job_id`, `company`, `title`, `urls`, and
`packets`. Treat `reference` as opaque. It is the retained Platter opportunity
key. Further preparation and regeneration preserve it. Each packet contains
its run `id`, UTC `created_at`, `preparation_status`, and `has_resume`.
`has_resume` means that a retained PDF artifact exists. These fields do not
establish application submission or employer response.

The read includes every opportunity with at least one retained preparation run,
including declined, stale, deferred and ineligible records. A job left by a
retrieval failure before preparation has no run and is excluded. URLs come from
the captured job and posting. Missing historical URLs remain absent. No source
text, career material, resume bytes or delivery body is returned.

Text search matches references, Cast IDs, companies, titles and URLs without
case sensitivity. An HTTP URL query uses exact supported ATS identity or Cast's
URL normalization. Supported ATS application-page suffixes and tracking queries
can identify the same posting. Text matches are candidates, not a unique-choice
guarantee. Empty results mean no retained match.

Each command reads one SQLite transaction at the canonical Platter root. It
opens state read-only and performs no preparation, source request, dependency
call, eligibility change, edition freeze or send. Closing a public posting does
not prevent this read. Platter preparation dependencies need not be ready.

The canonical database must exist at schema six. Missing or incompatible state
fails; follow the [installation contract](install-operate.md). The interface
does not repair or initialize state. Results cover retained records only, not
current employer availability. Full reads use memory proportional to the
retained library; no size or latency guarantee is supplied.

Keep returned URLs and job interests private. CLI dispatch attempts metadata-only
Chancery usage observation. It preserves the read result if observation fails.
Run `platter --register-usage` separately after installation to register commands.
