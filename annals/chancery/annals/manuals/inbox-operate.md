# Operate the Annals inbox

The Annals inbox is a durable filesystem spool driven by external scheduling.
Annals has no resident inbox daemon and every job has at most one processing
attempt. Operate it through supported commands; never edit `job.json`, move
terminal envelopes back to the queue, or infer delivery state from processes.

## Observe and admit work

```sh
/Users/joey/.local/bin/annals inbox status
/Users/joey/.local/bin/annals inbox register
/Users/joey/.local/bin/annals inbox enqueue <FILE>
/Users/joey/.local/bin/annals inbox enqueue --priority <FILE>
```

Registration moves settled top-level files from `incoming/` into complete
queued envelopes. It creates no database source-delivery record. Direct
enqueue copies explicit regular files into complete envelopes and leaves the
originals unchanged, but rejects a copy that would cross the configured spool
storage reserve. Priority is a binary lane; it does not renumber jobs and does
not preempt a processing job.

Dispatch is explicit or externally scheduled:

```sh
/Users/joey/.local/bin/annals inbox run
```

Scheduling is outside this cross-platform inbox contract. The macOS
installation contract documents its scheduler binding and lifecycle.

Before every new claim, Annals checks the configured available-byte reserve on
both the library and spool filesystems. Low storage leaves the next job queued
with attempts zero and no delivery record; the ordinary activation exits and a
later scheduled or explicit run checks again without requiring `resume`. A
storage probe failure also leaves the job unattempted but exits nonzero.

## Storage recovery authority and scope

Low storage is a safe stop, not cleanup authority. Preserve the queued work and
report the affected paths and available bytes. Neither the Annals liaison nor
an operating model or agent may, as storage remediation, delete, truncate,
rotate, prune, move, compress, overwrite, or otherwise clear user data, or
lower or disable the configured reserve, without the user's explicit consent
for the exact target and scope. A request to run, continue, retry, update, or
deploy Annals authorizes only that operation's documented effects, not
additional cleanup. Scheduled rechecks only observe capacity and may resume
ordinary queued work after the configured reserve becomes available.

The gate applies to new ordinary and retry-child claims. The related
direct-enqueue headroom check applies separately to an explicit spool copy. A
gated inbox item does not submit its associated liaison job to Nucleus. The gate
is not a host-wide lock and does not itself block an Annals deployment,
globally stop the Nucleus service or independent Nucleus jobs, or block another
product's deployment; manual `annals integrate` is also outside this gate.
Actual filesystem exhaustion can still make any operation sharing that storage
fail when it needs to stage a release, create a backup, migrate a database, or
write state or logs. An unreadable probe is distinct from measured low space
and can make deployment status inspection fail with `storage_probe_failed`.

When storage is ready, Annals performs an authenticated Nucleus account
preflight. Failure leaves the next job queued with attempts zero and no
delivery record. A claimed job moves to processing, increments from zero to
one attempt, and begins its source delivery. New work enters integration with
immediate application. Fresh exact-byte duplicates stop at retention without
a new examination.

## Pause, ordering, and interruption

```sh
/Users/joey/.local/bin/annals inbox pause
/Users/joey/.local/bin/annals inbox resume
/Users/joey/.local/bin/annals inbox prioritize <JOB_ID>
/Users/joey/.local/bin/annals inbox deprioritize <JOB_ID>
/Users/joey/.local/bin/annals inbox interrupt <JOB_ID> --as failed
```

Pause prevents a new dispatch claim after any active job finishes, but
registration continues and explicit enqueue remains subject to the storage
reserve. Resume removes only the
operator-owned pause and does not start a worker or clear deployment
maintenance. Prioritize/deprioritize accept queued ordinary jobs only.

Interruption names one exact processing job and records either `failed` or
`skipped`. It does not pause successors, so pause first when the next job must
remain queued. A skipped job receipt corresponds to a failed source delivery
with the dedicated skipped error; keep those lifecycle namespaces distinct.

## Bounded retry events

Retry is an explicit recovery event over an inclusive interval in failed
source-delivery completion order:

```sh
/Users/joey/.local/bin/annals inbox retry preview \
  --from <FAILED_JOB_ID> --through <FAILED_JOB_ID>
/Users/joey/.local/bin/annals inbox pause
/Users/joey/.local/bin/annals inbox retry start \
  --from <FAILED_JOB_ID> --through <FAILED_JOB_ID>
/Users/joey/.local/bin/annals inbox retry status <EVENT_ID>
/Users/joey/.local/bin/annals inbox retry continue <EVENT_ID>
```

There is no retry-all or open-ended event. Preview is read-only and reports
eligibility. Start requires the inbox paused, no processing job, no unfinished
event, and no maintenance. It freezes exact membership, preserves every
original job and delivery, and creates a fresh linked retry child per member.
A child still has one attempt. Only failures with retained-work identity are
eligible; correct a pre-retention source error and deliver it as new input.

An unexpected model/runtime failure or interruption halts the event. Continue
advances only unattempted items and never retries a failed child. The durable
event report, rather than process exit alone, is the accounting authority.
Insufficient or unreadable storage also halts an attended retry before claiming
the next child; correct the condition and continue that event explicitly.

Spool archives retain unchanged source material. Nucleus state may retain the
complete model request and output. Treat both as private. A Nucleus restart
makes an unfinished attempt lost; Annals, not Nucleus, decides subsequent
domain recovery.
