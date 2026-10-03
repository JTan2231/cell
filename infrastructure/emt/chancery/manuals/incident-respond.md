# EMT incident response

EMT owns Clockwork incident tracking and personal email exchanges. Its retained domain records are
incidents and exchanges. Each exchange carries one Nucleus job ID. Nucleus owns execution, output and tool activity. EMT has no agent-run
table, operation ledger, product-operation adapters or structured diagnosis.

## Authority

Enable EMT only when automatic investigation, account receiving reads and
incident emails are authorized. Each recognized non-automatic reply then
authorizes its one-off intervention. Sender verification is deliberately
deferred. Reply routes correlate incidents; they do not authenticate a person.
Quoted mail and diagnostic evidence do not add authorization. Email always
sends to its fixed personal recipient.

Clockwork owns the halt and exact incident-bound continuation. Agents check
current state before acting. An older reply does not approve a newer halt.
Products own recovery and domain success. Clockwork resume does not retry an
item, enable a disabled binding or undo committed work.

## Interfaces and record meaning

~~~sh
emt --json status
emt incident list
emt incident show INCIDENT_ID
emt run show EXCHANGE_ID
emt worker
emt pause
emt resume
~~~

Status counts retained incidents, exchange states and uncertain emails. These
are not lifetime totals or product-health verdicts. Incident show returns
correspondence and job references. Run show reads an exchange, including its
pending submission buffer. These explicit reads can expose private content.
Worker output uses counts and bounded waiting codes.

Use `emt.service` for worker admission, pause, schedule, configuration,
installation and maintenance behavior. Use `emt.installation.operate` for the
procedures that prepare and activate those interfaces.

A worker imports one page of 100 incidents and reads at most four 100-record
receiving pages. Incident creation is idempotent. The feed cursor advances
after every item on its page is stored. Initial discovery retains historical
incident metadata but diagnoses only open incidents that pass Clockwork's
shared service-check threshold. New pending failure episodes have no incident
and do not enter this feed before confirmation. Preexisting incidents below
their notification threshold remain available for later checks. EMT excludes
its own emt/worker halt from diagnosis.

Receiving fetches bodies only for recognized routes. Provider IDs deduplicate
replies; RFC Message-IDs thread responses. Cursors are scan positions, not
acknowledgements. Completed scans restart at the first page. A failed cursor
read resets the next scan. No account snapshot, completeness or arrival-time
guarantee is claimed.

## Agent assignments

One EMT job runs at a time. Queued replies take priority over queued diagnoses.
Each fresh assignment receives the incident, available selected definition,
basic notification, prior correspondence, Nucleus references and current email.

The default model is gpt-5.6-terra with medium reasoning. Another configured
model must be accepted by the installed Nucleus adapter. Jobs use unrestricted
current-user execution through Nucleus invocation policy version two, with
local execution and no built-in web search. The Codex sandbox does not restrict
filesystem, process, local socket, or network access, and approval prompts are
disabled. Operating-system permissions still apply. The default working
directory is the user's home; Cell source is supplied separately. EMT requires
the `workspace-unrestricted` capability before submission.

Agents use Chancery to discover contracts and invoke supported interfaces
directly. EMT has no command allowlist or operation adapters. Diagnosis is
instructed to investigate without recovery changes. The current reply
authorizes its intervention. This distinction is an agent instruction,
not a separate tool-enforcement layer.

The agent writes its own email and sends it through:

~~~sh
emt --json send EXCHANGE_ID --subject 'Subject' < BODY_FILE
~~~

EMT freezes the email on the exchange before transport, supplies the saved
reply route and calls Email's installed wrapper. Subject is a nonempty single
line of at most 256 bytes. Body is nonempty UTF-8 of at most 64 KiB. Agent mail
is accepted only while the exchange is running and before its deadline. A
different subject or body is refused after freezing. The receipt establishes
Resend acceptance, not inbox delivery. The agent does not send a separate copy.

Diagnosis expires five minutes after Clockwork records alert eligibility.
Waiting for the service-check threshold does not consume that deadline.
Reply assignments expire
15 minutes after the provider receipt timestamp. Nucleus active limits are
300 and 900 seconds; queue time does not extend EMT deadlines. On observing
expiry, EMT requests cancellation and waits for terminal state before another
assignment. The agent also receives the absolute deadline. Worker outages
can delay cancellation. Cancellation cannot undo an action already performed.

## Initial alert ownership

Clockwork gates new scheduling halts, basic alerts and new EMT diagnoses on
five consecutive
failed read-only service checks, at least 60 seconds apart by default. A healthy
check resets progress. Explicit inactive intent or operator pause excludes the
service and clears a pending failure episode. Later scheduled activations remain
admissible before the threshold; an established halt still requires exact
approval. Unknown health counts as failed with an explicit
unknown condition. Historical domain outcomes do not count as service failures.
`clockwork notification show INCIDENT_ID` exposes the count, threshold, last
check, condition and eligibility time before or after the threshold is reached.

The existing EMT worker advances due checks through Clockwork using its
configured Cell root. Checks use the installed bounded Iatreion report. They do
not run or retry product work. At the threshold, a check establishes the halt
and alert eligibility together. A resumed
incident suppresses an unalerted diagnosis. A continuous alert episode creates
no replacement diagnosis; admitted delivery and claims keep their recovery rules.

Clockwork defers new EMT-routed basic notifications for 120 seconds after the
threshold is reached. EMT
persists the agent-authored email before claiming initial-notification ownership
with the exchange delivery ID. Clockwork serializes claims with its basic
sender. A claim does not expire. EMT owns subsequent delivery recovery;
Clockwork retains the halt and claim without a diagnosis body.

When the basic send has already started, EMT sends its report as a follow-up.
The basic alert's incident Reply-To also routes to EMT. Clockwork exposes its
basic payload through notification show. EMT retains that payload with the
incident and keeps the Clockwork submission status distinct from receipt.

EMT's own worker halt always uses the basic path. There is no recursive
diagnosis. Nucleus unavailability does not prevent Clockwork's basic send.
If no broker runs, notification progress waits for another broker or an
explicit clockwork notification send. Provider and final-delivery latency
remain unspecified.

## Recovery and retention

The exact Nucleus request is saved on the exchange before submission.
Ambiguous admission reuses that request and ID. Confirmed admission clears
EMT's request buffer; the job ID and digest remain. Later reads verify
requester identity and digest. EMT creates no automatic replacement job.

Failed, lost or expired assignments become failed exchanges. If the agent
wrote no email, EMT prepares basic failure text with the job reference. A
retained agent email and receipt survive later runtime failure. EMT does not
infer which external operations succeeded. Inspect Nucleus and product state
before repeating an action.

Emails retain exact payloads and exchange keys. At most two transport
invocations occur, at least five minutes apart and within 23 hours of the
first invocation. Email can perform its own bounded HTTP retries. Exhausted
or expired ambiguous submission becomes uncertain. No new send identity is
generated. An uncertain email does not imply failed product work.

Private configuration and state remain under Application Support/EMT. There
is no automatic deletion of correspondence or job references. Nucleus, Resend
and the inbox provider retain independent copies. EMT does not mirror raw
logs or tool activity, copy credentials or erase provider records. Model
prompts, explicit exchange reads and email disclose their selected content.

## Quota and worker service

Quota can defer new assignments without creating a Clockwork failure halt.
Existing exchange deadlines still apply. `emt.quota-notices` owns the detailed
quota admission and shared-notice contract, including notice delivery without a
Nucleus job. `emt.service` owns worker scheduling, configuration, maintenance,
readiness and retained installation state. These are required feature contracts.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

## Prompt selection

New request preparation requires initialized private Bazaar state and a complete
`cell.prompts.emt` selection. The default database is
`~/.local/share/bazaar/bazaar.sqlite3`; an absolute `CELL_BAZAAR_DATABASE` override
selects another database. Reads fail without creating state or using embedded
fallback text. Missing or invalid selection stops preparation before admission.

The selection has content
`{"schema_version":1,"entries":{"PROMPT_ID":VERSION}}`. Every component version
is a positive integer. Component text and the complete selection are separately
published immutable versions. A text append alone does not change the selected
set. Selection version 1 and its referenced text versions preserve the migration
baseline. Runtime reads never import seeds; deployment supplies no missing text.

Bazaar loads the exact component versions and owns shared selection format,
prompt template rendering and trusted reference expansion through
`bazaar.prompts.prepare`. EMT owns authored meaning, selected components,
runtime inputs, request assembly and agent policy. EMT freezes the resulting
instructions in its saved request. Retrying that request retains its selection.
Later edits do not rewrite saved work or add execution authority.
See `emt.installation.operate` for
prompt prerequisites and `emt.development.change` for an authorized prompt-edit
procedure.

## Compatibility and limits

EMT schema one, Clockwork routing-sidecar version one, the provider release and
entry contract versions have separate compatibility rules. Replace old pinned
Clockwork brokers before enabling EMT routing; they cannot honor EMT claims.
Unsupported EMT schemas fail rather than being interpreted as current state.

No inbox completeness, authenticated sender identity, model accuracy, timer
latency, final-delivery deadline, future support lifetime or retention horizon
is promised. EMT does not authorize unrequested recipients, unrelated changes,
automatic failed-job replacement or clearing a newer Clockwork halt.
