# EMT quota deferral and shared notices

EMT observes Nucleus weekly-quota conditions, defers new model assignments and
sends one retained deterministic email for each new shared condition. This
feature applies across requesters; it does not require an EMT incident or a
Nucleus job to author or send the notice. Nucleus owns quota observations and
admission policy. Email owns transport and credentials.

Use this feature to interpret quota waiting, a shared condition notice or an
uncertain notice submission. The worker advances this feature through
`emt worker`; `emt --json status` provides the ordinary retained-state view.
There is no dedicated quota-notice CLI or automatic operator retry command.
Use `emt.installation.operate` for authorized worker activation and recovery.

## Admission and record meaning

The worker reads Nucleus `GET /v1/quota` without starting an agent. While quota
blocks admission, unadmitted exchanges wait until recovery or their existing
deadline. Expired quota deferrals and `quota_exhausted` attempts retain their
outcome without an individual fallback failure email. Retained agent emails
continue through ordinary exchange delivery. Unrelated incidents retain their
normal handling. Quota waiting does not create a Clockwork failure halt or clear
existing pauses and failure halts.

A missing or unavailable observation postpones new model work while frozen mail
delivery continues. An old daemon's quota-endpoint 404 permits rollout without a
quota gate. Worker recovery and operator pause stop discovery of new notices
but allow frozen notice delivery. Observed quota is Nucleus's account observation,
rather than an EMT billing or completion guarantee. No quota-refresh, timer,
account-availability or final-delivery deadline is promised.

## Notice identity and delivery

Each new shared condition freezes one deterministic email in the private
`quota-notifications/CONDITION_ID.json` record under EMT's state root. The
Nucleus condition ID identifies the notice; `emt/quota/CONDITION_ID` is the exact
Email idempotency key. Email sends only to its fixed personal recipient. A quota
notice does not grant authority for a product intervention or new agent attempt.

The frozen payload has at most two transport invocations, at least five minutes
apart and within 23 hours of the first attempt. Email has its own bounded HTTP
retry behavior. An acceptance receipt ends sending; it proves provider acceptance
rather than final inbox delivery. An unresolved exhausted send remains uncertain
for inspection. EMT generates no new send identity to escape uncertainty.

## Recovery and privacy

Keep the frozen record, payload, receipt and key through restarts. Preserve
`quota-notifications/` through maintenance. Do not delete a record to retry mail.
Inspect retained state and Email evidence before repeating any action. A worker
restart or quota recovery cannot undo an email already accepted by the provider.
A missing or uncertain notice does not establish failure of requester work.

These private records are separate from EMT incident and exchange tables and
Nucleus runtime history. EMT stores no Email credentials. The provider and inbox
retain independent copies. There is no automatic deletion or promised future retention horizon. External
idempotency policy belongs to `email.message.send`; EMT makes no additional
idempotency-lifetime or final-delivery promise.

## Compatibility and authority

The EMT release, schema-one domain state, quota endpoint and Email contract
versions have separate compatibility rules. The exact saved condition and mail
identity survive worker recovery. `emt.service` explains installed state and
worker admission; `emt.incident.respond` owns exchange deadlines and assignment
recovery. These related references add no authority or dependency cycle.

Activate the worker only when account observations and condition mail are
authorized. Reading this feature does not authorize installation, activation,
unrequested recipients, quota-policy changes or unrelated product mutations.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time and thread ID, not arguments, output or
outcomes. Internal product calls are excluded. Recording errors do not change
command results.
