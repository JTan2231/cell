# Run a renderer and email its stdout

Paperboy runs a configured command, captures stdout, and submits successful
nonempty output to Email as a plain-text body. Scripts own collection, source
access, report structure, and data windows. Paperboy adds no report text.

Use this capability when execution and email submission are authorized. A
scheduled job requires standing authority for its script and resulting email.
Reading configuration or preparing schedules does not execute the script.

## Select and inspect a manifest

The default manifest is
`~/Library/Application Support/Paperboy/paperboy.toml`. Use an absolute
`--manifest` path to select another file:

```sh
paperboy init
paperboy list
paperboy doctor
paperboy --manifest /absolute/paperboy.toml list
paperboy --manifest /absolute/paperboy.toml run daily-report
```

`init` writes an empty manifest only when the selected file is absent. It
validates and preserves existing configuration. `list` reads configured jobs,
not execution history. `doctor` validates configuration and required executable paths
without running a renderer or sending email. It does not certify script output,
source access, Email credentials, or future timer delivery.

## Manifest format

The manifest is a regular UTF-8 TOML file of at most 1 MiB. It requires version
one and a `jobs` table. Unknown fields are rejected:

```toml
version = 1

[jobs.daily-report]
render = ["/absolute/path/render-report", "--daily"]
subject = "Daily report"

[jobs.daily-report.schedule]
kind = "local-calendar"
hour = 9
minute = 0

[jobs.hourly-report]
render = ["/absolute/path/render-hourly"]

[jobs.hourly-report.schedule]
kind = "interval"
seconds = 3600
```

Each job ID begins with a lowercase ASCII letter. The remaining characters are
lowercase ASCII letters, digits, or hyphens. Its maximum length is 63 bytes.
The ID identifies both the manifest entry and its `paperboy/JOB_ID` Clockwork
binding. Renaming an ID creates a different job.

`render` is a nonempty array of literal strings. Its first value is an absolute
executable path. Arguments receive no shell expansion, interpolation, globbing,
or automatic shell interpretation. Arguments must contain no NUL. To run an
interpreted script, supply its interpreter and script path explicitly, or use an executable script with a
supported shebang. The literal argument `-c` is unsupported; put command logic
in a script file.

`subject` is optional. Its default is the exact job ID. It is separate from
stdout and becomes Email's subject. An explicit subject must be nonblank and
contain no control characters. The manifest contains no recipient or
body template. Email fixes the sender and recipient.

Each job requires exactly one schedule. `interval` accepts `seconds` from 1
through 31,536,000. `local-calendar` accepts `hour` from 0 through 23 and
`minute` from 0 through 59. A local-calendar job uses the host's local timezone.
Both forms register with `run_at_load = false`. Paperboy exposes no timezone,
catch-up, or occurrence-window setting. Clockwork and launchd own timer delivery;
the renderer owns which data period to report.

## Execute and send

```sh
paperboy run daily-report
```

Manual execution reads the selected manifest and executes that job once. It
uses the renderer executable's parent directory as its working directory.
The renderer receives only `HOME`, `PATH`, and `CHANCERY_USAGE_INTERNAL=1`.
`PATH` is `/usr/bin:/bin:/usr/sbin:/sbin:HOME/.local/bin`, with `HOME` replaced
by the user's absolute home path. Other caller environment variables are
cleared. Shell startup files are not sourced by Paperboy.
Renderer stdin is closed; Paperboy supplies no input stream.

A renderer is a trusted normal-user program. It has the current user's
filesystem, process, and network access under operating-system permissions.
Paperboy supplies no sandbox or source-specific access policy. A script must
arrange its own configuration and any source authentication.

Paperboy waits for successful process exit before submitting output. A nonzero
exit, timeout, invalid UTF-8 stdout, or stdout above 64,000 bytes fails the run
and sends no email. Paperboy rejects an oversized body rather than truncating
it. Stderr is separate and never becomes the email body.

Successful stdout with zero bytes skips Email. Whitespace-only output is
nonempty and is sent unchanged. Paperboy preserves spaces, line endings, and
trailing newlines. It does not parse stdout into headers, remove commentary,
format Markdown, or convert it to HTML.

The command returns `job_id`, `outcome`, and `body_bytes`. Empty output returns
`outcome = "skipped_empty"`. Acceptance returns `outcome = "accepted"` and
`provider_message_id`. The command does not print the email body.
A receipt proves that Resend accepted submission. It proves neither final
inbox delivery nor source accuracy or completeness.

Commands print formatted JSON data by default. `--json` selects a compact
`{"ok":true,"data":...}` envelope. With that flag, failures write
`{"ok":false,"error":"..."}` to stderr and exit nonzero. Without it, errors
use the `paperboy: ` prefix. `status-snapshot --json` returns the raw version-one
Iatreion snapshot rather than this envelope.

## Schedule snapshots

Use `paperboy apply` and the controls in `paperboy.install.operate` to register
and select schedules. New jobs remain disabled until explicitly enabled.

An applied definition fixes the installed Paperboy executable, job ID,
subject, renderer argv, absolute Email wrapper path, schedule, and launch context.
The scheduled runner uses that snapshot without reading the manifest again.
Manifest edits affect manual runs immediately; apply them to update schedules.
Applying a definition does not freeze renderer file bytes or source data.
Clockwork verifies its registered top-level Paperboy launch image, not the
renderer or its dependencies.

## Failure and repeat execution

Paperboy retains no rendered body, report record, occurrence ledger, structured
send ledger, or retry queue. Clockwork retains scheduled activation history.
Product logs can retain execution results, including an accepted message ID,
and bounded diagnostics. They contain no captured renderer stdout and provide
no payload recovery or resend interface.

Paperboy invokes Email once for one successful nonempty rendering. Email can
retry transport within that invocation under `email.message.send`. Paperboy
performs no process retry. A later manual or scheduled execution runs the
script again and submits its new output. It does not resume the prior message
or preserve a prior idempotency key. Scripts own any collection checkpoints,
side effects, or duplicate suppression they need.

A timeout or interrupted Email call can leave submission acceptance unknown.
An error does not establish nonacceptance. Inspect Resend before an explicit
new execution when duplicate submission matters. Paperboy provides no
reconciliation command and cannot reconstruct the prior body.

Each scheduled definition declares `halt-until-approved`. Clockwork ends a
failed activation and applies its shared service-health delay before an
established halt. Later activations can occur while that failure episode is
pending. Resolve `clockwork.schedule.operate` for the complete policy, incident
notification, and exact continuation rules. Approval permits future scheduling;
it does not retry a failed Paperboy run.

## Limits and privacy

| Operation | Limit |
| --- | --- |
| Manifest file | 1 MiB |
| Renderer execution | 1,200 seconds |
| Successful stdout body | 64,000 UTF-8 bytes |
| Retained stderr diagnostic prefix | 4,096 bytes |
| Email wrapper observation | 120 seconds |
| Clockwork activation | 1,380 seconds |

Paperboy drains stderr beyond its diagnostic limit and discards the excess.
Diagnostics can contain private data written by the script to stderr.
Captured stdout remains in memory for validation and submission. Email
receives the exact subject and body and discloses them to Resend and Gmail.
Email's selected wrapper owns credential loading. No credential belongs in
manifest arguments, subjects, or Clockwork definitions.

Configuration, executable paths, arguments, schedules, and logs are private
local data. Clockwork retains definition and activation metadata but ingests
no renderer output. Scripts and providers own their separate retention.
No source freshness, throughput, start delay, completion time, or inbox arrival
objective is promised.

## Compatibility and command usage

Paperboy 0.3.0 uses this manifest-render contract and TOML version one. It does
not open or migrate the retired
`~/Library/Application Support/Paperboy/paperboy.sqlite` database. Old briefs,
agent requests, summaries, and send attempts remain untouched. The new CLI
provides no legacy report, preview, retry, reconciliation, or maintenance
operations. Retained legacy programs and upstream state keep their own rules;
see `paperboy.install.operate` before cutover or recovery.

Contract five replaces the conversation and decision-report interface. Scripts
can use other installed products under those products' contracts. Paperboy
itself depends only on Clockwork and Email for scheduling and submission.
No general future compatibility window or deprecation period is promised.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors preserve
command results. Runtime commands do not invoke Chancery discovery.
