# Daily application email

Clew renders a plain-text snapshot of every currently tracked application except
those whose current supplied status is `rejected`, after trimming whitespace
and comparing without case sensitivity. Other status text is shown unchanged.
A missing status appears as `No status recorded`. A notes-only report does not
change status. Retractions and replacements use the same current-record rules
as `clew list`. Only explicit application reports contribute status and notes.
Generic ledger notes and plain `--ref cast.job JOB_ID` links do not appear in the
application view or email, even when they share a thread with an application report.

Each included application shows company, role, supplied status and all retained
job URLs. Items sort by company, then role, without case sensitivity, with the
opaque Cast job ID as the final tie-breaker. Saved notes follow the list,
grouped in the same application order. Every active note appears unchanged in
ledger sequence order. Superseded and retracted records, and retraction
explanations, are excluded. Rejected applications contribute no counts or notes.

The report is a full daily snapshot, including when records have not changed.
An empty snapshot says `No applications to show.` No model, application-age
calculation, correspondence read, status inference or application write occurs.
The ledger's `recorded_at` records reporting time, not application time.

Clew reads the ledger once and joins Cast's complete retained snapshot by exact
job ID. These are separate snapshots. A Cast read failure or missing job leaves
each qualifying application visible under its Cast job ID, with missing job
details marked and one context-unavailable footer.
A ledger read failure stops rendering. An empty selected list needs no Cast
read. Retained links do not establish that a posting is still open.

## Preview and send

```sh
clew email preview
clew --json email preview
clew email send
clew email send --scheduled
```

Preview prints From, To, Subject and body. JSON returns `data.digest` with
`subject`, `body`, `application_count`, `context_available` and
`ledger_sequence`. The sequence is the last ledger append observed, including
corrections; it is null for an empty ledger. Context is available when every
included application has Cast metadata. It is also true for an empty list.
Preview creates no delivery state and sends nothing.

Manual send requires explicit authorization. Enabling `clew/daily-email` grants
standing authority for this exact daily content to Email's fixed personal
recipient. Preview, installation and definition generation grant no send
authority. The installed Email wrapper owns credentials, fixed addresses and
bounded transport retries. A returned `accepted_id` proves provider acceptance,
not final inbox delivery. Complete selected statuses, job details, links and
notes are disclosed to Resend and Gmail. No attachments are sent.

Clew retains an exact message, a random stable idempotency key, first-attempt
time and acceptance receipt in private `email.sqlite3` beside `ledger.sqlite3`.
The email database is schema one and is created on the first send; it does not
change the schema-three ledger. Files use mode 0600 under the private
0700 state directory. Records have no automatic pruning. Clew supplies no data backup or restore.

Sends hold product admission and a separate email lock. A manual occurrence is
`manual/UUIDv7`. A scheduled occurrence is `daily/YYYY-MM-DD` for the most recent
local 09:00. The subject uses the rendering date. A repeated scheduled call
returns an accepted occurrence without another submission. Manual sends do not
consume the daily occurrence. There is no backlog replay of missed dates.

## Recover uncertain submission

Clew freezes the message before submission and records attempt admission before
calling Email. Interruption, a transport error or a failure to save the receipt
can leave acceptance uncertain. Scheduled calls refuse another submission of
that occurrence.

1. Inspect Resend to establish whether the message was accepted.
2. Read the frozen message with `clew email preview --occurrence ID`.
3. Run `clew email send --retry ID` only when another submission is authorized.

Retry uses the original message and key, even after statuses or notes change.
Ledger migration does not rewrite frozen messages or send keys.
It is allowed for less than 23 hours after the first attempt, and is refused
after a backwards clock change. Email's external idempotency window is 24 hours.
An accepted occurrence returns its receipt without resubmission. After the safe
window, inspect provider acceptance before authorizing a new manual occurrence.
An unknown occurrence fails. `--retry` and `--scheduled` cannot be combined.

## Schedule and failure behavior

Generate a definition after installing Clew and initializing its ledger:

```sh
clew-install schedule-definition --state-dir ABS_STATE --output ABS_NEW_FILE
```

The definition pins the installed release and uses `clew/daily-email`, local
09:00, no run-at-load, skipped overlap, a 180-second limit and
`halt-until-approved`. Generation writes a new private definition and prepares
private log paths; it does not register or enable the binding. See the
`clew.install.operate` procedure for coordinated setup.

Login, sleep and launchd affect actual activation. No start-delay, catch-up,
message-size, provider-availability or inbox-delivery guarantee is supplied.
All selected applications and notes are retained in the message; provider size
limits can cause failure and do not authorize truncation.

A scheduled failure ends the current send and starts a pending Clockwork failure
episode. Later scheduled activations remain admissible before the shared
service-health threshold. By default, five consecutive failed read-only checks,
at least 60 seconds apart, halt the binding and make its alert eligible together.
Healthy or inactive checks clear a pending episode. Checks do not resubmit an
uncertain occurrence. Inspect `clockwork incident list clew/daily-email`.
Resolve the cause and any uncertain
submission before explicitly approving
`clockwork binding resume clew/daily-email INCIDENT_ID`. Continuation permits
future activations; it does not retry an uncertain message. Deliberate admission
during deployment maintenance returns a successful skip. Schema-three ledger read and
record commands remain available during email maintenance.

CLI dispatch attempts metadata-only Chancery usage recording. It records no
status, note, email body or credential, and does not change domain success.
