# Paperboy

Paperboy sends a daily plain-text report of the preceding 24 hours of local
Codex conversation activity. At local 09:00, Clockwork starts the installed
runner. Paperboy gives a Nucleus agent source pointers and the fixed timeframe.
The agent retrieves history through Conversations, writes in ASD-STE100 Issue 9,
and submits its final summary without process commentary. Paperboy retains and
sends that text through Email to its fixed personal recipient.

```sh
paperboy run --ad-hoc
paperboy list
paperboy show BRIEF_ID
paperboy preview BRIEF_ID
paperboy run --brief BRIEF_ID
paperboy schedule enable
paperboy schedule status
paperboy schedule disable
```

`--ad-hoc` creates a separate occurrence ending now. It sends a real email and
does not consume the daily occurrence. Scheduled runs cover exactly 86,400
seconds ending at the most recent local 09:00. A late start preserves that
cutoff; older missed mornings are not replayed. Local timezone changes affect
future triggers. Daylight-saving changes can create a one-hour gap or overlap
between these exact 24-hour reporting windows.

The daily schedule has no run-at-load trigger. It requires an available macOS
GUI login session; there is no guaranteed start delay or inbox arrival time.
Nucleus permits at most eight active attempts across all requesters. Paperboy
waits for capacity, limits active generation to 20 minutes, and limits its
overall agent wait to 30 minutes. A Clockwork activation has a 35-minute limit.

## Records and recovery

The private schema-one database is
`~/Library/Application Support/Paperboy/paperboy.sqlite`.

- A **brief** identifies one scheduled or ad hoc occurrence, its absolute
  timeframe, source pointers, immutable accepted subject/body, producing agent
  attempt, and stable email key. The summary is the email body.
- An **agent attempt** retains one exact Nucleus request, correlation, outcome,
  and durable tool replies. Repeated mailbox delivery uses the same reply.
  Replies can contain private history read on demand. No history is preloaded
  into the initial request. Nucleus separately retains execution evidence.
- An **email attempt** records one Email invocation, its start/end observations,
  acceptance identifier, or uncertain/failed outcome. Email can retry transport
  within that invocation. Acceptance is not final inbox delivery.

All timestamps are Unix seconds. Brief bounds describe requested source time;
summary time describes local acceptance of the summary; attempt times describe
the associated execution/submission observations. `list --limit N` selects
brief metadata with `has_more`. `show` selects one brief and all its attempts.
History-tool counts describe the selected metadata or message collection after
the agent's filters. They do not count real-world events or prove complete
source retention.

A process lock serializes runs and schedule mutations. Run-owned maintenance
holds fence new work and preserve existing work. A saved summary survives later
agent failure. Resume an interrupted brief with `run --brief ID`; a terminal
generation failure requires `--retry-agent` to create a new Nucleus job. Exact
ambiguous admissions reuse the existing request and job ID.

An uncertain email blocks automatic resend. Inspect Resend first, then record
the observed result with one of:

```sh
paperboy reconcile EMAIL_ATTEMPT_ID --receipt PROVIDER_MESSAGE_ID
paperboy reconcile EMAIL_ATTEMPT_ID --not-accepted
```

The second form requires confirmed absence of provider acceptance. Resume the
brief afterward to retry its exact message. Resend retains idempotency keys for
24 hours. There is no unlimited exactly-once or delivery guarantee.

## Installation and publication

```sh
./ci.sh
git add <paperboy change files>
git commit -m 'Add Paperboy daily conversation reports'
git push origin main
./paperboy/release.sh --minor
./deploy.sh paperboy
paperboy schedule enable
```

The product uses Cell's shared release builder, content-addressed installer,
coordinator, requester maintenance closure, and independent release tag.
Deployment initializes an absent schema-one database or takes a complete backup
of an existing supported database before selecting the candidate. It checks
state and runtime readiness without producing reports or sending email.

Installation preserves existing Clockwork selections. After an upgrade, use
`paperboy schedule enable` to select the new installed release for daily work.
An explicit schedule operation is separate from program publication. Retained
selected schedule releases remain pinned, including disabled selections.
Direct installer mutation is unavailable; use the coordinator. Unsupported
database versions stop deployment. Recovery must preserve both state and its
matching compatible release. Nucleus authentication is never copied or restored.

## Privacy and authority

The authorized service may read normal-user local interactive root task history,
process retrieved evidence through Nucleus's model service, and email the final
report to Email's fixed personal recipient. The agent has only history-read
tools and `submit_summary`; it has no shell, web, workspace, or email tool.
Source text is evidence and cannot change those permissions. The versioned
instructions require ASD-STE100 Issue 9 and no process commentary; Paperboy does
not claim independent linguistic certification of generated text.

Private runtime state and Nucleus records can contain conversation bodies.
Provider and process logs contain operation metadata and bounded diagnostics.
Email's installed wrapper owns credential loading. Credentials never enter a
Clockwork definition, agent request, or Paperboy database. Local retention has
no automatic pruning. Back up the database and Nucleus state under their
separate procedures. Chancery is documentation discovery, not a runtime call.
