# Direct activation and runtime evidence

Use this feature to run the selected definition once or interpret Clockwork
process evidence. Clockwork owns admission, one directly supervised child,
process identity, and runtime history. Each product owns durable work,
idempotency, retries, output content and retention, recovery, and domain success.
A zero child exit remains `exited`; it does not become a product success claim.

## Interfaces and output

```text
clockwork [--json] run KEY
clockwork [--json] history [KEY] [--limit N] [--details]
clockwork [--json] doctor
clockwork [--json] status-snapshot
```

Public commands print plain text by default. `--json` selects the existing
compact `{"ok":true,"data":...}` envelope. The `status-snapshot --json`
interface retains its raw Iatreion snapshot schema without that envelope.
Omit the flag for a plain-text view of the same snapshot.
With the flag, a coded failure writes
`{"ok":false,"error":{"code":"...","message":"..."}}` to stderr and exits
1. Other failures are human-readable on stderr. Machine callers must pass
`--json`.

The private launchd entry point retains its existing JSON broker receipts.
Product stdout and stderr still go to the registered product-owned files.
Public output selection changes neither those files nor their content.
Run accepts only an `owner/name` key, with no runtime executable, argv,
environment, cwd, schedule, timeout, or policy.

## Admission and supervision

Both `clockwork run KEY` and the private launchd entry point resolve only a
stable key. Callers cannot supply executable paths, arguments, environment,
working directory, schedule, timeout, or overlap policy at runtime.

A launchd process waiting at the transition gate has not yet been admitted and
does not pin a definition. After the gate opens it resolves the then-current
binding. Definition identity is pinned only when Clockwork commits the running
activation row.

The broker enters a shared per-key transition gate, refuses a pending binding
transition, pins the selected immutable definition, acquires the exclusive
per-key activation lock, and re-verifies the direct artifacts. Schema-three launch images are fixed
runtime files whose bytes must still match the retained archive and definition. It then starts
the same installed Clockwork binary as a blocked execution gate and new
process group leader, records that PID, and only then releases a one-byte
parent pipe.

EOF before that release makes the gate exit without product execution. After
release the gate proves that its PID owns the running activation, re-verifies
the definition, claims and unlinks a private status marker, opens the product
outputs, and `exec`s the registered program or `/bin/sh` profile in place.

A pre-exec or loader failure is written only to that marker and becomes
`start_failed`; Clockwork diagnostics do not enter product output. The
recorded PID is therefore the product PID after exec, and there is no
unrecorded spawned-product window.

The broker waits for that process, forwards termination to its process group,
applies the optional timeout, and records one terminal result. One activation
is one product execution; there is no attempt/retry submodel. The hidden gate
accepts only key plus activation identity and cannot execute unless its own PID
matches the already committed running row.

Terminal states are `start_failed`, `exited`, `signaled`, `timed_out`,
`skipped_overlap`, and `lost`. `running` is nonterminal. Every overlap is
recorded as `skipped_overlap` without starting a child. A scheduled overlap
returns success to launchd; a manual busy run returns `activation_busy` with
exit 1. An unfinished row is
changed to `lost` only after its recorded broker and any recorded child are
both demonstrably absent.

Clockwork records no stdout or stderr body. It opens the definition's validated
distinct product-owned destinations for append, or creates them mode 0600,
and verifies the opened device/inode identities differ. The product owns their
content and retention. `clockwork.definitions` owns destination path and
permission admission rules.

Once a child starts and Clockwork durably records `exited`, `signaled`, or
`timed_out`, the broker exits zero even for a nonzero child exit. With
`--json`, it emits `ok:true`. Admission, validation, output-open, spawn,
persistence, manual-busy, and broker failures exit one. With `--json`, they
emit `ok:false`. A pre-start failure after admission
is retained as `start_failed` when the terminal write succeeds.

A supervision failure after spawn retains the observed terminal state when
cleanup is proved. If termination or persistence cannot be proved, the row
remains `running` for conservative lost recovery. Diagnose both runtime and
product evidence before authorizing new work. Clockwork never retries a child.

## History, identity, and freshness

History selects newest activations first, optionally for one stable key.
Output-version-two pages contain `items` and `has_more`, default to 20 rows,
and accept a positive `--limit` for a larger prefix. Rows contain activation
ID, key, trigger (`manual` or `launchd`), timestamps, state, exit code, signal,
and failure detail. `--details` adds definition digest and known process IDs.
The definition and process identities describe this exact activation.

Timestamps use whole Unix seconds. PIDs identify local broker and direct
child processes. The direct child is its process-group leader. Interval and
optional timeout values use 1 through 31,536,000 whole seconds. The records
contain no stdout/stderr body, product-domain result, or retry plan. Reads
expose retained local evidence at invocation and supply no service-level
promise for completion, history retention, throughput, or storage capacity.

## Diagnosis and shared status reads

`doctor` opens the schema-two store, enforces private owned state paths, runs
SQLite `quick_check`, resolves the current Clockwork executable, and verifies
`/bin/launchctl` is present. It also marks a retained `running` activation
`lost` when both its recorded broker and any recorded child are demonstrably
absent, reports the count, and lists pending binding-transition journals.
Doctor does not repair those journals because `binding switch` may restore a
run-at-load service while `binding disable` resolves directly to an inactive
selection. Invoke the intended operation for that exact key under the
product's maintenance gate. Doctor does not execute a job, switch a binding,
inspect product state, interpret domain success, prove future timer delivery,
or establish a launchd availability SLA.

Iatreion uses `clockwork status-snapshot --json`. It opens only an existing
supported database read-only and reports binding, halt, and recorded runtime
metadata for joining to explicit Cell units. The additive
`scheduler_observations.failure_pending` boolean distinguishes a pending failure
episode from an established halt; pending failure leaves admission open.
It does not initialize or migrate
state, reconcile running rows, inspect launchd, claim notifications, or change
a binding. Runtime records do not establish a current product-domain condition.

## State, access, and privacy

The normal database is
`$HOME/Library/Application Support/Clockwork/clockwork.db`. No command falls
back to state in the current directory. Private database and directories,
foreign keys, transactions, a bounded busy timeout, and per-key filesystem
locks protect local consistency. Product durable work belongs outside this
transaction. Direct SQLite integration is unsupported.

The hidden absolute `--state-root` override is for controlled tests and
isolation. It is not a second production authority. Definitions and activation
history expose private paths, arguments, schedules, digests, and process times.
Keep state and logs private. Clockwork stores no product secret or
output body. There is no daemon, HTTP surface, agent execution, workflow,
retry engine, distributed coordination, or system/root service.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Recording failure does not change the command result. Internal
calls, `__launchd`, and `__exec` are excluded. Clockwork adds
`CHANCERY_USAGE_INTERNAL=1` to the child after applying the registered environment.

## Compatibility and related features

Output pages use version two; manifest schema, store schema, feature contract,
provider release, and product definitions evolve independently. Incompatible
meaning requires a successor schema with historical decoding and explicit
migration. No general deprecation interval or cross-release support window is
promised.

`clockwork::api` owns the activation/history types, codecs, and a typed client
for an explicitly selected executable. Read `clockwork.definitions` for
artifact admission, `clockwork.bindings` for selection and launchd limits,
`clockwork.incidents` for failure enforcement, and `clockwork.notifications`
for eligible alert effects around broker visits.
