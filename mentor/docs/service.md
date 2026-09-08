# Mentor email practice

Mentor owns one personal daily practice loop: send a problem, receive a complete
answer, send a critique. It uses the authored problem and rubric captured for
that assignment. The desktop Mentor app remains the authoring location; the
mail service neither reads nor edits its draft database.

## Daily selection

Initialization imports the bundled corpus, creates a shuffle seed, and leaves
admission paused. The defaults are 09:00, `America/Chicago`, the configured
Resend receiving domain, and `~/.local/bin/email` as the transport wrapper.

After the configured local time, a worker can reserve one problem for the
current calendar date. It selects an unused stable problem ID from a
deterministic shuffled order. Reservation and its frozen outgoing message
commit together. Reservation consumes the problem ID even if submission later
fails; another pass cannot silently substitute a different exercise for that
date. No previous missed dates are backfilled. When no unused IDs remain in the
active corpus, selection produces no new message and status reports zero
unused problems.

The title is the email subject. The body is the complete authored problem
Markdown, with no greeting, hints, rubric, or appended instructions. A random
128-bit token produces an assignment-specific Reply-To mailbox of the form
`mentor.<32-hex-token>@RECEIVING_DOMAIN`. Mentor retains that mapping and the
exact corpus digest. Refreshing the corpus does not change an old assignment
or reset the set of used IDs.

## Receiving and grading

The worker reads received account email through Email's installed-wrapper
client. It visits at most four pages of 100 metadata records per pass, retaining
the last provider cursor to continue an unfinished scan. A completed scan
resets the cursor for the next scan. Cursors are not acknowledgements. Each
incoming provider ID is deduplicated against Mentor's retained incoming records.
An expired or failed cursor read resets scanning to the first page.

Mentor fetches full content only for a recognized assignment address. Both the
opaque token and the full saved address must match. It compares the parsed
sender with Email's fixed personal recipient and excludes recognized automated
messages. These checks limit routing; they do not establish cryptographic
sender authentication. Incoming From and Reply-To values never select an
outbound recipient.

A usable reply must have a valid RFC Message-ID and a provider receipt timestamp.
An answer may address an old assignment, but remains eligible for processing
only until 24 hours after that incoming email's provider timestamp. Reading
old mail after that deadline does not create a new grading window.

Mentor accepts a complete answer in the body. It prefers plain text and can
extract text from HTML locally without loading external resources. It removes
common quoted history and signature separators. It asks for a complete pasted
answer when attached content, an empty answer, excessive length, or ambiguous
inline quoting prevents reliable extraction. Those replies are processing
guidance, not grades. Unrelated mail and recognized automated responses do not
receive a response.

Each grading request contains only the assigned problem, its frozen rubric,
and the current extracted answer. There is no prior answer or critique context.
Nucleus runs `gpt-5.6-terra` with medium reasoning, a 1,200-second active limit,
workspace access `none`, no local execution, no web, and no dynamic tools or
Email credential. Mentor requires the exact supported invocation capabilities.
An accepted Nucleus job can wait for one of its eight execution slots; that
wait is separate from its active timeout and does not extend Mentor's deadline.

The requested result is a concise qualitative critique: what works, material
gaps or mistakes, and relevant tradeoffs. It must not contain a numeric score,
letter grade, pass/fail verdict, replacement design, or new exercise. Mentor
accepts only a matching completed job with a nonempty final result within
64 KiB. The output instructions govern content; the size and correlated
completion checks are mechanical. A failed attempt produces a short request to
reply again rather than an automatic replacement model attempt. A new grading
job waits until cancellation of an expired prior job is confirmed.

The critique uses the original problem title with `Re:` and the incoming RFC
Message-ID and References chain. Email supplies the headers and fixed addresses.
The email client decides thread display. A later reply is another independent
answer, not a conversation continuation.

## Records and retention

The canonical state root is
`~/Library/Application Support/MentorMail`. Its schema-one SQLite database is
`mentor.sqlite3`. Private directories and files, an exclusive runner lock,
full synchronous commits, an in-memory temporary store, rollback journaling,
and SQLite secure deletion protect the local processing boundary. These do
not control provider records or copies outside the database.

| Record | Meaning and lifetime |
| --- | --- |
| Corpus | Exact problem/rubric export, identified by digest. Retained to interpret old assignments. |
| Assignment | Reserved local date, problem ID, corpus digest, and reply-token mapping. Retained for no repeats and late answers. It does not prove email acceptance. |
| Incoming metadata | Provider/RFC IDs, assignment link, receipt time, state, Nucleus correlation, and error code. Terminal metadata is eligible for removal after 35 days from provider receipt. |
| Pending answer/request | Temporary content needed to submit or rediscover the exact grading request. Replaced or cleared as work advances; expires 24 hours after provider receipt. |
| Pending outgoing payload | Exact problem, critique, or processing-guidance email and key. Cleared on recorded Resend acceptance or expiry. A reply shares its incoming email's deadline; a daily problem has 24 hours from reservation. |
| Outgoing receipt metadata | Submission state, Resend receipt when known, attempt timestamps, and error code. Terminal records are eligible for removal after 35 days from creation. |
| Cancellation correlation | Job ID and pending-cancellation flag. Kept until the Nucleus job is terminal or absent, even if normal metadata expiry has passed. |

Cleanup runs at worker entry and exit. A stopped worker cannot enforce an exact
wall-clock deletion deadline. Once a critique submission receipt commits,
Mentor clears its outgoing content and any remaining answer/request buffers
in the same transaction. There is no command to browse prior answers or grades.
Unresolved work and cancellation metadata can remain past 35 days; the content
deadline still applies.

Nucleus retains its own request and exact execution output. Its current
contract provides no general pruning API. Resend and the inbox provider retain
email under their own policies. Mentor cleanup does not erase those copies or
this conversation. Explicit migration backups are allowed only after all work
and cancellation have drained and no pending content remains.

## Submission and recovery

Mentor persists exact Nucleus requests before admission. An ambiguous admission
reuses the same job ID and byte-equivalent typed request. It validates returned
requester/job identity before consuming a result. Nucleus failure does not
authorize a new model attempt under another ID.

Before each email attempt Mentor retains the frozen payload, idempotency key,
and first-attempt time. Failed or uncertain submissions retry that same payload
with increasing delay, capped at one hour. The retry window ends 23 hours after
the first attempt, or at the content deadline if earlier. It stops short of
Resend's documented 24-hour idempotency retention. An unattempted expired
message becomes failed; one with an attempt and no confirmed receipt becomes
unknown. No new payload/key is generated to conceal that uncertainty.

A recorded Resend receipt means submission acceptance, not Gmail delivery or
successful threading. A worker completion means that a pass ended; inspect its
error codes and retained outgoing states separately. Restart recovery uses the
private records rather than rereading a prior critique into product history.

## Commands and observations

Add `--json` to request the compact machine envelope:
`{"ok":true,"data":...}`. Failures use `ok:false` and an error detail.
Status and worker output contain counts, timestamps and bounded error codes,
never problem, answer or critique content.

```sh
mentor init
mentor configure --time 09:00 --timezone America/Chicago
mentor configure --receiving-domain example.resend.app \
  --email-executable /Users/joey/.local/bin/email
mentor pause
mentor resume
mentor --json status
mentor --json doctor
mentor import-corpus /absolute/corpus.json
mentor tick
mentor worker
mentor schedule enable
mentor schedule disable
mentor schedule status
mentor maintenance hold OWNER
mentor maintenance status
mentor maintenance drain
mentor maintenance release OWNER
mentor migrate --backup /absolute/private/backup.sqlite3
```

Configuration accepts a lowercase DNS receiving domain only when the complete
`mentor.<32-hex-token>@domain` mailbox fits Email's 254-byte limit. The Email
path is absolute; Mentor does not source a shell profile or read its credential.

Initialization, configuration, corpus import, pause/resume, tick, schedule,
maintenance and migration are separate effects. `tick` and `worker` may submit
emails and model jobs under the configured practice authority. `pause` stops
new assignments and receiving admission; existing work and expiry can still
advance. It does not revoke pending submissions. A maintenance hold stops new
admission; drain performs one bounded recovery pass for existing work. Repeat
drain as needed and inspect its result before releasing that owner's hold.

`status` distinguishes `assigned_problem_count` from
`unused_problems_in_active_corpus`. Incoming and outgoing state counts cover
retained records only, so they decrease after metadata cleanup and are not
lifetime grading totals. `last_poll_completed_at` identifies the last completed
full provider scan. `last_tick_completed_at` identifies the last completed
worker pass, which may report stage errors. These timestamps use Unix seconds
UTC; the daily date uses the configured IANA time zone.

`doctor` inspects local installation/state compatibility and whether the Email
executable is present. It explicitly reports Email API/receiving permission,
Nucleus execution readiness and live delivery as `not_probed`. It sends no mail
and starts no model job. Schedule status describes the selected
Clockwork binding, not Mentor's admission pause or completion of a practice
operation.

`migrate --backup PATH` refuses an existing database with unresolved work,
cancellation, or remaining answer/request/outgoing payload text. It copies
only drained state to a private 0600 file. An existing backup must match the
drained database exactly; it is never overwritten. With no database yet,
migration initializes paused schema-one state and reports `backup:null`.

The standard `mentor/worker` definition runs every 60 seconds with no
run-at-load, skips overlap, and limits one activation to 90 seconds. A tick
advances bounded stages and does not wait for a model to finish. Activation,
network, model, and inbox delays have no guaranteed bound. See
[installation](system-installation.md) for supported release, binding, and
maintenance operations.
