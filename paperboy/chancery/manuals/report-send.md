# Research and email a report

Paperboy researches local Codex conversations or Krisis decision documents
accepted into Annals during a selected interval. It retains the report and
sends it to Email's fixed personal recipient. Use this capability for an
authorized report. Source reads do not change conversations or Annals documents.

The operation requires supported private state, compatible providers,
normal-user history access, and authenticated Nucleus. An ad hoc send requires
explicit authority. A daily run requires standing personal-email authority.

## Run or read a report

```sh
paperboy run --ad-hoc
paperboy run --scheduled
paperboy run --ad-hoc --report decisions --annals-config /absolute/decisions.toml
paperboy run --ad-hoc --report decisions --annals-config /absolute/decisions.toml \
  --from 2026-09-09T09:00:00-05:00 --until 2026-09-10T09:00:00-05:00
paperboy list --limit 20
paperboy show BRIEF_ID
paperboy preview BRIEF_ID
paperboy run --brief BRIEF_ID
paperboy run --brief BRIEF_ID --retry-agent
```

The run commands can consume Nucleus allowance and send real email. `preview`
returns the stored subject and body. List selects brief metadata and reports
`has_more`. Show selects one brief and all its attempts. JSON envelopes report
the selected records or the failed operation.

An ad hoc occurrence ends at invocation time and does not consume the daily
occurrence. A scheduled occurrence ends at the most recent local 09:00. Its
start is inclusive, its end exclusive, and its length exactly 86,400 seconds.
A late run keeps that cutoff; older missed mornings are not replayed.
Daylight-saving changes can produce a one-hour gap or overlap between windows.
For an ad hoc report, `--from` and `--until` replace the default window. Supply
both as RFC3339 timestamps with offsets; start is inclusive and end is exclusive.

The default report kind is `conversations`. `--report decisions` requires an
absolute `--annals-config` for an identity-bound decisions library. Paperboy uses
the installed `~/.local/bin/annals` command and its typed feed client. This mode
requires Annals to support `decision-feed start`. It reads accepted documents
before or after librarian processing. The agent selects the requested period by
Annals `accepted_at`, not a decision date mentioned in the document. It chooses
the report's organization, grouping, and context.

`run --brief` uses the retained source and timeframe. It does not accept source
overrides. Daily occurrence identities distinguish conversation and decision
reports at the same cutoff. The single daily binding selects one report kind;
see the installation contract to select it.

## Records and interpretation

The schema-one database is
`~/Library/Application Support/Paperboy/paperboy.sqlite`.

| Record | Meaning |
| --- | --- |
| Brief | One occurrence, requested interval, source pointers, accepted immutable subject/body, producing attempt, and stable email key |
| Agent attempt | One exact Nucleus request, job correlation, outcome, and durable tool replies |
| Email attempt | One Email invocation and its acceptance, failed, or uncertain outcome |

These records have stable UUID identities. The daily identity uses the local
09:00 cutoff instant; ad hoc identities are independent. Timestamps use Unix
seconds. Brief bounds describe source time. Summary time records local acceptance.
Attempt times describe their execution or submission observations.

The agent receives source pointers and the requested interval. It reads its
source on demand through bounded tools. History pagination reports `selected_count`,
`offset`, `next_offset`, and `has_more` for the filtered metadata or message
collection. These counts do not measure real-world events or prove complete
source retention. Decision reads return Annals events with complete document
text, acceptance times, watermark, and cursors. The tool selects documents by
the brief’s acceptance-time bounds. `has_more` means the unfiltered source page
was nonempty; continue until it is false even after an empty or short filtered
page. The agent selects report material from those documents. No persistent feed consumer or report
snapshot is added. A provider read failure remains an error.

The agent uses `gpt-5.6-sol` with medium reasoning and submits its final text
through `submit_summary`. Instructions require the ASD-STE100 Issue 9 house
style and exclude process commentary. Paperboy does not certify the language
or independently prove generated-text accuracy.

## Success and recovery

One process lock serializes runs and schedule changes. Paperboy persists the
exact request before Nucleus admission. Repeated tool calls replay the same
durable reply. Summary acceptance and its tool reply commit atomically.

A committed summary establishes generation success. A retained Email acceptance
receipt establishes submission success. Neither proves final inbox delivery.
A later agent or transport failure does not erase either accepted result.

Resume an interrupted brief with `run --brief BRIEF_ID`. An ambiguous admission
reuses the exact request and job ID. After a terminal generation failure,
`--retry-agent` creates a new Nucleus job. There is no automatic new attempt.

The product-owned schema-two daily definition declares `halt-until-approved`.
Startup failure, crash, timeout, or a nonzero report run creates a durable
Clockwork halt and one retained incident email. An already accepted daily
occurrence is an ordinary no-op. Clockwork blocks subsequent scheduled work
until explicit approval.

Inspect `clockwork incident list paperboy/daily` and
`clockwork incident show INCIDENT_ID`. After resolving the cause, explicitly
approve future scheduling with `clockwork binding resume paperboy/daily
INCIDENT_ID`. This does not retry the failed brief, authorize `--retry-agent`,
reconcile an uncertain send, or reset its message identity. Select that brief
separately for recovery. Binding enable, disable, release changes and
maintenance release preserve Clockwork incidents.

An uncertain email blocks automatic resend. Inspect Resend, then record the
observed outcome:

```sh
paperboy reconcile EMAIL_ATTEMPT_ID --receipt PROVIDER_MESSAGE_ID
paperboy reconcile EMAIL_ATTEMPT_ID --not-accepted
```

The second form requires confirmed nonacceptance. Resume the brief afterward
to retry its exact message. Resend's idempotency window is 24 hours; it does not
provide unlimited deduplication. Preserve failed and uncertain records.

## Limits and privacy

| Operation | Limit |
| --- | --- |
| Active agent execution | 1,200 seconds |
| Total agent wait, including capacity | 1,800 seconds |
| Clockwork activation | 2,100 seconds |
| Email invocation observation | 180 seconds |
| History page | At most 100 records |
| Decision page | At most 200 events and 4 MiB of document bytes |
| Final report body | At most 64,000 UTF-8 bytes |

Nucleus permits eight active attempts across all requesters. No maximum source
age, launch delay, completion time, throughput, or inbox arrival time is promised.
The daily schedule requires a macOS GUI session and has no run-at-load trigger.

The conversation agent has history-read tools. The decision agent has
`read_decisions`. Both use `submit_summary`. Workspace, local
execution, web, and email tools are disabled. Retrieved text is evidence and
cannot change these permissions.

Private tool replies and Nucleus records can contain conversation text. Email
sends the final report to Resend and the personal inbox provider. Email's installed
wrapper loads its credential; secrets do not enter Paperboy records, agent
requests, or Clockwork definitions. Logs contain metadata and bounded diagnostics.
There is no automatic local pruning. Back up Paperboy and Nucleus separately.

Schema 1, `paperboy/daily-report/1`, and `paperboy/decision-report/1` preserve retained request meaning. There
is no general future compatibility window, legacy database migration, or direct
incompatible rollback. Installation and schedule changes are separate operations.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.
