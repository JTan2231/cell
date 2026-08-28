# Architecture

Todo has one synchronous executable and one local SQLite database. It owns no
daemon, network service, usage sidecar, or background queue. The optional
macOS schedule is a launchd timer that invokes the same executable; it is not a
resident Todo process. Model execution is delegated to the separately
installed, per-user Nucleus service.

Deterministic commands (`list`, `search`, `show`, `note add`, `done`, and
`reopen`) access SQLite directly. `new` additionally submits a Nucleus job and
services its session-scoped `create_todo` tool. A successful, validated tool
call is the creation result; the liaison's final prose is only diagnostic.

## Creation flow

1. Resolve the source to an absolute path and validate that it names a readable
   UTF-8 file. Todo does not retain the bytes or copy them into the initial
   prompt; the liaison reads the path during its research. Nucleus retains the
   resulting raw agent protocol, which may include content Codex reads or
   emits, under Nucleus's own local retention boundary.
2. Submit a Nucleus job rooted at the caller's working directory with broad
   read access.
3. Supply the source path, caller working directory, direction, and the stable
   research prompt.
4. Allow the liaison to inspect the source and pursue relevant local or web
   research.
5. Validate one `create_todo(title, note)` call and insert its content with
   host-supplied provenance, status, and timestamps in one transaction.

The liaison is intentionally not confined to the source. References in it are
research leads, and the liaison may inspect code, documentation, tests, Git
history, other conversations, and external materials when they materially
improve scope or completion criteria.

Read access is not guarded by a Todo-specific allowlist. The process may use
the normal research facilities available to Codex. Its write boundary is
strict: project files and external systems are read-only, and the only
state-changing application tool exposed by Todo is `create_todo`. The liaison
does not receive shell-based write authority or general-purpose external
mutation tools.

Todo sends the caller's environment to Nucleus as a single-use, memory-only
launch context so local research observes the same shell environment as the
invoking command. Nucleus removes inherited `CODEX_EXEC_SERVER_URL`, supplies
its own isolated file-backed authentication, and does not persist the launch
environment with the job. Todo never reads or copies Codex credentials and
does not select a remote Code Mode host.

The host, not the liaison, supplies `pointer`, `source_path`, `status`, and
timestamps. A second creation call is rejected. If Codex stops without a
successful call, no todo is created. If the insertion commits and Codex then
fails while returning its diagnostic prose, the durable creation wins and is
reported.

Research is proportionate rather than mechanically bounded. The prompt tells
the liaison to stop when more investigation is unlikely to materially improve
the intended outcome, current-state understanding, affected parties and
systems, constraints, dependencies, or verification criteria.

SQLite uses foreign keys, a busy timeout, WAL journaling, read-only connections
for reads, and immediate transactions for writes. These are operational
details, not additional domain concepts.

## Outstanding-todo email

The email commands are a deterministic read projection and do not invoke a
liaison. They require an `[email]` configuration containing one sender and one
recipient, read every currently open todo, and render one subject plus plain
text and HTML bodies. Entries are newest-first and contain only public ID and
title; notes, pointers, sources, timestamps, and completed todos are excluded.
An empty open set still renders and sends an all-caught-up digest.

`email preview` returns that exact rendered request without reading an API key
or making a network request. `email send` renders once and immediately posts
the request to Resend using `RESEND_API_KEY`. A manual send uses
`todo-email/<UUIDv7>`. `email send --scheduled` is still immediate, but uses
`todo-daily-email/<LOCAL YYYY-MM-DD>` for the most recent 09:00 occurrence.
The request deliberately has no Resend `scheduled_at`; launchd owns recurrence
and local-time semantics.

One invocation reuses the exact rendered body and idempotency key for up to
three total attempts, retrying transport failures, `429`, and `5xx` responses.
Version 1 stores no delivery row, retry queue, or rendered snapshot in SQLite.
Resend retains idempotency keys for 24 hours, so another invocation for the
same local 09:00 occurrence with changed todo content can be rejected as a
conflicting reuse rather than silently sending twice.

Email sending reads Todo's database and talks directly to Resend. It does not
submit a Nucleus job, use Codex authentication, or depend on Nucleus health.
Todo text leaves the local security boundary in both the email and Resend's
service. Resend documents
[30-day retention](https://resend.com/docs/knowledge-base/account-quotas-and-limits)
of email content and metadata; its optional
[no-content-storage feature](https://resend.com/docs/knowledge-base/how-do-i-ensure-sensitive-data-isnt-stored-on-resend)
has separate plan and eligibility requirements.
