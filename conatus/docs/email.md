# Daily wants email

Conatus sends every active want in a plain-text email. It copies `wording`
unchanged. New messages omit source-reference fields and show the capture date
for each want and quotation as `YYYY-MM-DD`, with no label or time. Dates use
UTC. Stored sources and capture times remain unchanged. Items are ordered by
capture time and ID, newest first. The email has no want limit, inferred
priority, rewritten title, or model invocation.

For each want, Conatus reads concepts grounded by its work and their direct
children through Annals' supported read interface. It follows every root,
child and evidence page at one fixed corpus revision. Evidence on a child
identifies candidate source works. Conatus selects the two newest distinct
captured decision records among those works, ordered by capture time and ID.
It copies one existing quotation per decision. Ties between quotations use
concept ID order, then Annals evidence order. A path through intermediate
concepts does not qualify. Acceptance and associations do not prove progress.

The complete local intake read and the Annals revision are separate snapshots.
An active want appears even if interpretation is pending. Failure to read
supporting material produces the full active wants list with one
context-unavailable footer. Failure to read local intake stops rendering. An empty list says
`No active wants.` Stored wording and quotations are never truncated.
Provider message-size limits can reject a send; they do not authorize omission.

Archive and unarchive affect newly rendered messages. Selection uses the local
state snapshot at rendering. A frozen occurrence keeps its original bytes for
retained preview and explicit retry, including any source fields and capture
timestamps from an older format or wants archived afterward.

## Preview and send

```sh
conatus email preview
conatus --json email preview
conatus email send
conatus email send --scheduled
```

Preview reads current state and prints From, To, Subject and body. JSON returns
`data.digest` with subject, body, want count, context availability and the Annals
revision when available. Preview changes no domain state and sends nothing.
It does not authorize sending. Manual send requires explicit authorization.
Enabling `conatus/daily-email` grants standing authority for this exact daily
content to Email's fixed personal recipient.

Send uses the installed `$HOME/.local/bin/email` client. Email owns credential
loading, its fixed sender and recipient, and bounded transport retries. Sending
discloses complete want wording, selected quotations and their capture dates
to Resend and the recipient provider. Retrying an older frozen message also
discloses any source fields and capture timestamps in its retained bytes.
No attachments are sent. A returned `accepted_id` proves provider acceptance,
not inbox delivery. The email command never runs `conatus update`.

Conatus retains each message in its existing private settings storage before
submission: occurrence, exact subject and body, library-scoped idempotency key,
first-attempt time and acceptance ID. A separate email lock serializes sends.
The intake schema remains version 1. A manual occurrence uses `manual/UUIDv7`;
a scheduled occurrence uses `daily/YYYY-MM-DD` for the most recent local 09:00.
An accepted scheduled occurrence is not submitted again. Ad hoc sends do not
consume the scheduled occurrence. Retained messages have no automatic pruning.

## Recover uncertain submission

Conatus records attempt admission before calling Email. Interruption, timeout,
transport failure or failure to save the acceptance receipt can leave acceptance
uncertain. Later scheduled calls refuse to resubmit that occurrence.

1. Inspect Resend to establish whether the message was accepted.
2. Inspect the retained bytes with `conatus email preview --occurrence ID`.
3. If another submission is needed, run `conatus email send --retry ID`.

Explicit retry uses only the retained bytes and key. It is allowed for less
than 23 hours after the first attempt and is refused after a backward clock
change. Email's external idempotency window is 24 hours. An accepted occurrence
returns its existing receipt without another send. After that window, inspect
provider acceptance before explicitly authorizing a new ad hoc occurrence;
Conatus does not retry the old occurrence automatically.

## Schedule and maintenance

Generate the separate schema-two Clockwork definition:

```sh
conatus-install schedule-definition --daily-email --state-dir ABS_STATE --output ABS_NEW_FILE
```

It uses `conatus/daily-email`, local 09:00, no run-at-load, skipped overlap,
a 180-second activation limit and `halt-until-approved`. The existing update
binding retains its independent schedule. Definition generation does not
register or activate either binding. Login, sleep and launchd control actual
activation; no maximum start delay or catch-up guarantee is provided.

Cell deployment settings accept `daily_email_enabled` separately from update
`enabled`. Omission preserves the prior daily-email selection and enabled
intent; an absent binding remains absent. Deployment captures, suspends,
retargets and restores both exact bindings. It preserves existing failure
halts. Send holds the product admission guard; deliberate scheduled admission
during deployment returns a successful maintenance skip.

Inspect `clockwork incident list conatus/daily-email` after a scheduled failure.
Resolve the cause and any uncertain acceptance before explicitly approving
`clockwork binding resume conatus/daily-email INCIDENT_ID`. Continuation permits
future scheduling; it does not retry an uncertain email or clear an update halt.
