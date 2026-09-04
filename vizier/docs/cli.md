# CLI and operation

All commands accept `--database ABSOLUTE_PATH` to select one private Vizier
ledger and `--json` for machine-readable output. Meaning-bearing inputs remain
files containing exact Markdown; `ID=FILE`, `NAME=COMMAND`, and identifiers are
mechanical routing syntax.

## Initialize and diagnose

```sh
vizier [--database ABS] [--json] init
vizier [--database ABS] [--json] doctor
```

`init` creates or idempotently accepts the supported ledger. `doctor` checks
the selected ledger and the exact Nucleus capabilities required by Vizier. It
does not submit implementation work or prove that a future run will complete.

## Submit

```sh
vizier [--database ABS] [--json] run submit \
  --repository ABSOLUTE_REPOSITORY \
  --brief FILE \
  --terminology FILE \
  --contract ID=FILE [--contract ID=FILE ...] \
  [--gate NAME=COMMAND ...] \
  [--source GIT_REVISION] \
  [--request-key KEY] \
  [--remediation-rounds N]
```

The repository, brief, terminology snapshot, and at least one caller-enumerated
contract unit are required. Contract order is preserved. `--source` selects
the exact Git basis; when omitted, Vizier resolves the documented current
repository basis at admission. A request key makes an exact resubmission
idempotent and rejects changed reuse.

Each gate has a stable name and one caller-authorized command. Gates execute
only at the integrated-candidate stage and are evidence for that candidate,
not release or deployment actions. Do not supply a command that pushes,
releases, deploys, changes requirements, or operates unrelated state.

The default automatic remediation allowance is one round. An explicitly
supplied value is a positive finite upper bound. Submit drives the run
synchronously and reports its durable run identity even when the process is
later interrupted.

## Inspect and recover a run

```sh
vizier [--database ABS] [--json] run list
vizier [--database ABS] [--json] run show RUN_ID
vizier [--database ABS] [--json] run status RUN_ID
vizier [--database ABS] [--json] run wait RUN_ID
vizier [--database ABS] [--json] run resume RUN_ID
vizier [--database ABS] [--json] run cancel RUN_ID
```

`show` and `status` select the exact durable run. `wait` observes and services
recoverable in-flight work. `resume` continues the stored request after an
interrupted coordinator, without changing its frozen documents or Git basis.
`cancel` records intent and requests cancellation of correlated active jobs;
it cannot undo an already committed Vizier record or source mutation.

Run states are `queued`, `planning`, `assembling`, `plan_review`,
`implementing`, `packet_review`, `integrating`, `gates`, `final_review`,
`succeeded`, `needs_attention`, and `cancelled`.

## Inspect and retry an attempt

```sh
vizier [--database ABS] [--json] attempt show ATTEMPT_ID
vizier [--database ABS] [--json] attempt retry ATTEMPT_ID
```

Inspect both Vizier and correlated Nucleus evidence before retrying. An
ambiguous admission reuses only the byte-equivalent persisted request and same
Nucleus job ID. `attempt retry` is an explicit successor attempt and is allowed
only when durable evidence makes another attempt safe. It never turns a
terminal Nucleus result into Vizier success and never creates an unbounded
retry loop.

## Exit and proof boundary

Machine-readable output is the supported automation surface. A successful
process invocation means only that its requested operation completed. Run
success belongs to the exact durable Vizier run and candidate evidence; use
`run show` after uncertainty.
