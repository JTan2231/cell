# EMT incident response

EMT owns Clockwork incident tracking and personal email exchanges. Its two
database tables are incidents and exchanges. Each exchange carries one Nucleus
job ID. Nucleus owns execution, output and tool activity. EMT has no agent-run
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

## Inspect and advance

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

Pause stops new incident and reply discovery and disables EMT preference for
newly routed Clockwork notifications. Admitted exchanges continue. Resume
validates configuration, configures Clockwork's EMT route and reopens admission.
Neither operation enables a schedule or clears a Clockwork halt.

A worker imports one page of 100 incidents and reads at most four 100-record
receiving pages. Incident creation is idempotent. The feed cursor advances
after every item on its page is stored. Initial discovery retains historical
incident metadata but diagnoses only open incidents. EMT excludes its own
emt/worker halt from diagnosis.

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
model must be accepted by the installed Nucleus adapter. Jobs use read-write
workspace access, local execution and no built-in web search. The default
agent working directory is the current user's home so Cell operations and
EMT's mail command can write their user-owned state. Cell source is supplied
separately. Actual access remains subject to the Nucleus sandbox.

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

Diagnosis expires five minutes after EMT capture. Reply assignments expire
15 minutes after the provider receipt timestamp. Nucleus active limits are
300 and 900 seconds; queue time does not extend EMT deadlines. On observing
expiry, EMT requests cancellation and waits for terminal state before another
assignment. The agent also receives the absolute deadline. Worker outages
can delay cancellation. Cancellation cannot undo an action already performed.

## Initial alert ownership

Clockwork defers new EMT-routed basic notifications for 120 seconds. EMT
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

## Shared quota notices

The worker reads Nucleus `GET /v1/quota` without starting an agent. While quota
blocks admission, unadmitted exchanges wait until recovery or their existing
deadline. Expired quota deferrals and `quota_exhausted` attempts retain their
outcome without generating an individual fallback failure email. Retained agent
emails still use the ordinary delivery path. Unrelated incidents retain their
normal handling. Existing pauses and failure halts are not cleared.

Each new shared condition freezes one deterministic email in the private
`quota-notifications/CONDITION_ID.json` record under EMT's state root. Email is
invoked directly with key `emt/quota/CONDITION_ID`; no Nucleus job authors or sends
this notice. The same frozen payload has at most two transport invocations,
at least five minutes apart and within 23 hours of the first attempt. A receipt
ends sending. An unresolved exhausted send remains uncertain for inspection.
Keep this directory in backups; do not delete records to retry delivery.
A missing or unavailable quota observation postpones new model work while frozen
email delivery continues. An old daemon's quota-endpoint 404 permits rollout
without a quota gate. Worker recovery and operator pause stop discovery of new
quota notices but allow frozen notice delivery.
