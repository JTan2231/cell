# Operate the serial Cell CI queue

Use `cell-ci` to submit one committed Cell input for unattended integration,
validation, bounded repair, exact-source deployment, and a personal outcome
email. The installed manager serves one configured Git common repository on
this host. Linked worktrees share that queue.

One delivery job is active at a time, including while it waits for a model,
deployment, or notification. There is never more than one automated repair
loop. The manager invokes the internal validator, which uses the shared broker
for individual gates.

The manager is shared infrastructure. It has no separate product gate,
release-publication operation, or automatic self-deployment target. Catalog
presence does not establish live service, model, deployment, or email readiness.

## Initialize and install

Select an existing commit that is known to be an acceptable starting baseline.
Initialization records that operator selection; it does not validate the commit
or prove which source is installed.

```sh
cell-ci init --repo /absolute/cell --accepted-baseline COMMIT
cell-ci install
cell-ci service start
cell-ci status
cell-ci resume
```

Use the repository wrapper to initialize or install this source package:

```sh
./ci.sh init --repo /absolute/cell --accepted-baseline COMMIT
./ci.sh install
```

The wrapper always runs initialization and installation from the source package.
It routes the other queue and service commands to the installed manager.
The root and product `ci.sh` wrappers provide this same manager path. Use
`./ci.sh submit COMMIT` to start CI. Bare invocations, product selection, and
direct validation flags are unsupported; they return submission guidance.
Product wrappers do not limit validation or deployment to that product.

Initialization starts with admission paused. It records the repository's common
Git directory, selected baseline, and policy. It does not infer a baseline from
the current `main` or from installed products. Do not initialize another queue
scope to bypass existing work.

Configure these optional values at first initialization when the default is
unsuitable. Each value must be a positive integer:

```sh
cell-ci init --repo /absolute/cell --accepted-baseline COMMIT \
  --luna-attempts 3 --terra-attempts 1 --model-timeout-seconds 600
```

The flag names are retained for compatibility. `--luna-attempts` sets the first
tier's Terra medium budget; `--terra-attempts` sets the escalation tier's Sol
high budget. Retained policy fields use the same legacy names. For new jobs,
these limits count unrefunded repair attempts, as described below.

Repeated initialization does not replace the repository or policy and cannot
move an accepted ref that has already advanced.

Python 3.11 or newer, Git, and the selected repository gates' tools are required.
Model repair also requires compatible Nucleus execution and the installed
Bazaar prompt selection `cell.prompts.ci-manager`. It pins versions of
`ci-manager.repair.instructions` and `ci-manager.repair.prompt`. Missing prompt
records fail; the worker does not create Bazaar state or use a source fallback.
Deployment and Email retain their own setup and readiness requirements.

Installation selects manager files and the matching `ci-manager` Chancery
provider and configures a macOS user service. The installed worker is separate
from the changing development checkout. A source edit alone does not replace
the running manager. Service start and stop do not clear queue pause, retained
jobs, maintenance owners, or unresolved effects.

Installation normally requires paused admission and no active job. Service stop
always requires both conditions. Installation has only the exception for
cancelled validation described below.
Installation keeps the queue paused and loads the service. It pins a
content-addressed release under `~/.local/share/cell-ci/releases/` and selects
the matching executable and provider through `current`. It refuses foreign
selectors or LaunchAgent files and attempts to restore the prior selection if
installation fails. A failed restoration leaves an explicit recovery error.
After stopping its owned service during installation or rollback, the installer
waits up to 10 seconds for the worker lock. It refuses to proceed if the lock
remains held, and retains all required checks under the lock after acquisition.

To install edited repository code, pause admission, let the active job finish,
and invoke that source package explicitly:

```sh
cell-ci pause
cell-ci status
./ci.sh install
cell-ci status
cell-ci resume
```

Calling `cell-ci install` from an installed release selects that release's own
bytes; it does not find newer source automatically. The repository's
`./ci.sh install` selects that checkout's manager package.

Installation can also replace the worker while one explicitly cancelled job is
blocked in its checking phase. Admission must be paused. The job must have no
pending recovery request, accepted source, model attempts, unresolved model
execution, or deployment. The exact retained validation supervisor request and
matching result must prove that the validation process exited. A missing result,
process disappearance, or an arbitrary blocked job does not qualify.

The installer verifies these conditions before stopping its owned service and
again under the worker lock. It replaces program and provider bytes while
preserving the job and its evidence. Installation does not complete the job,
accept its source, or establish successful validation.

After this installation, run `cell-ci recover JOB`. The new worker reconciles
the retained process result and recognizes cancellation before reading the
aggregate validation receipt. A missing aggregate therefore does not prevent
this cancellation from proceeding through normal outcome handling. Inspect the
retained outcome before resuming admission. The ordinary service-stop command
does not use this installation exception.

Install this manager before submitting a commit with the manager-only wrappers.
Workers older than 0.2.0 invoke the public root wrapper for validation and
cannot validate that commit. This worker invokes the candidate's internal
`pipeline/select_changes.py run` with the fixed base, candidate, and JSON receipt
arguments. Manager release 0.3.0 uses queue contract 3 and retains journal
schema 1. New submissions freeze `policy.refund_accepted_patches = true`.
Existing jobs without this flag retain their original policy, which charges
every invocation. Installation preserves the pause until an explicit resume.

## Submit a committed input

Commit the intended changes before submission. Then submit that commit:

```sh
./ci.sh submit COMMIT
cell-ci submit COMMIT
cell-ci submit COMMIT --repo /absolute/cell --request-id REQUEST_KEY
cell-ci submit COMMIT --deploy nucleus --deploy email
```

The repository option must identify the configured common Git repository.
Submission resolves and pins the input commit, freezes its policy and selected
deployment request, and returns a durable job ID. Caller exit does not abandon
an admitted job. Reuse the same request key for an uncertain submission of the
same payload; do not reuse it for changed input or deployment choices.

Submission authorizes the defined bounded patch loop, private candidate commits,
advancement of `refs/ci/accepted`, the selected exact-source deployment, and the
deterministic outcome email. It does not authorize remote Git publication,
credential changes, arbitrary emails, or deletion of retained work.

Repeat `--deploy PRODUCT` to select deployment products explicitly. Without that
selection, the manager uses products covered by the passing validation's product
and platform scope. The deployment coordinator may include its declared
companions. If no products are selected, the result states that deployment was
not required; it does not claim a fresh installation.

## Follow the serial source history

Development remains on `main`. The manager does not edit the submitter's files
or advance development `main`. It owns `refs/ci/accepted` and private input and
candidate refs below `refs/ci/jobs/JOB/`.

When a queued job is claimed, it captures the current accepted commit as its
base. It merges the submitted commit into that base in a private worktree and
commits the result. Every repair creates another private candidate commit. CI
compares each candidate against the same captured base, so earlier submitted
changes and repairs remain in the required scope.

A submitted commit includes its ancestry. A later descendant can include an
earlier failed change and receives its own job budget. Accepted repair commits
can be absent from development `main`. A later merge preserves those repairs
where Git can combine the histories. A merge conflict stops the job; the worker
does not ask a model to resolve it or choose either side automatically.

If the input is already an ancestor of accepted history, the job reports
`already_included`. It runs no new repair loop or deployment. This is an ancestry
result, not a request to restore the old commit's exact tree after later reverts.

The validator returns aggregate receipt schema 1 with the fixed base, exact
candidate, source identity, required gate scope, and completed gate receipts.
Selective CI remains selective. A pass does not establish that every repository
test ran or that an automated patch is semantically correct in all cases.

The manager advances accepted with an expected-old-value check only after
matching successful validation. A changed accepted ref stops promotion. Accepted
source and installed source are separate: a later deployment failure does not
erase an already accepted commit.

## Apply bounded model proposals

New jobs have a default budget of three unrefunded `gpt-5.6-terra` attempts
with medium reasoning, followed by one unrefunded `gpt-5.6-sol` attempt with high
reasoning. Each recorded attempt consumes one point. When Git accepts its patch
and the manager records the private candidate commit, that point is refunded.
The refund does not require a later validation pass.

Failed or rejected attempts remain charged for the whole job. A different
validation failure after an accepted patch does not erase those earlier
charges. The next model tier follows the unrefunded count: successful Terra
patches keep Terra available until three attempts remain charged; successful
Sol patches keep the final point available. One invocation can propose fixes
for several diagnostics and files.

This policy bounds unrefunded attempts, not total invocations, runtime, or cost.
Accepted patches can continue beyond four total invocations. There is no
separate total-invocation ceiling. Every invocation keeps a new, monotonically
increasing attempt number and unique provider identity. A refund never deletes
history or reuses an earlier invocation. Jobs whose frozen policy lacks
`refund_accepted_patches` retain the original limit on all invocations.

The manager invokes Nucleus with requester program `ci-manager`, a unique saved
job identity, `workspaceAccess: read-only`, ordinary local shell execution
enabled, and web search disabled. The agent reads the candidate source, Cell
contracts, retained failure diagnostics, and prior attempt context. Nucleus
enforces the sandbox. The manager supplies no custom reading or patching tools.

The manager asks for a raw Git patch in the final response, without explanation,
Markdown fences, a JSON envelope, or reasoning text. It saves that exact final
response. It never uses reasoning, execution logs, or another output channel as
the patch proposal.

Git decides whether the proposal applies. The manager adds a missing final LF
to the application input, then runs `git apply --cached --recount
--whitespace=nowarn` against a private index initialized from the recorded parent.
Git derives hunk counts from the patch body. The manager adds no patch grammar,
size, byte, path, file-type, or changed-content restrictions to Git's rules.

Successful Git application produces the tree for the next private candidate
commit, which goes through the ordinary CI loop. Recording that candidate
establishes the refund for jobs with the refund policy. A Git rejection keeps
its attempt charged and can proceed to the next permitted attempt. The agent
never applies the patch, commits, runs the managed CI loop, deploys, or sends
email.

The default model execution timeout is 600 seconds. Nucleus owns execution-slot
and quota waiting; those waits do not grant another job or a larger repair
budget. The manager preserves the exact prepared request on admission deferral
or uncertain transport, without charging that attempt again. A terminal model
execution failure keeps its charge and is not automatically a useful patch.
Unknown or lost execution remains unresolved until supported provider evidence
permits recovery.

## Observe, pause, cancel, and recover

Read retained queue or selected-job state:

```sh
cell-ci status
cell-ci status JOB
cell-ci wait JOB --timeout 60
```

Status reports manager records and their last observed provider evidence. Queue
status includes the active job and queued jobs in sequence order; select a job
ID to read its retained terminal record. It is not a live probe of every provider.
Job and enqueue sequence identify retained work; commit IDs identify input, base
and candidate. Record timestamps and attempt stamps use Unix seconds. Wait
observes the same job and does not submit a replacement. Its optional timeout
uses seconds and returns a timeout observation without cancelling the job.

Each job status includes `repair_budget`, derived from its frozen policy and
retained attempt history:

| Field | Meaning |
| --- | --- |
| `mode` | `refund_accepted` for the refund policy; `invocations` for the original policy. |
| `total` | Sum of the first-tier and escalation budgets; four by default. |
| `used` | Recorded attempts minus refunded attempts. |
| `remaining` | Budget points still available. |
| `refunded` | Attempts with a recorded private candidate that receive a refund under the frozen policy; zero in `invocations` mode. |
| `invocations` | Total recorded attempts, including those whose points were refunded. |

Budget exhaustion does not discard an accepted patch: its candidate still goes
through validation. Cancellation, unknown execution, and recovery retain their
existing stop and reconciliation rules.

Control queue admission and cancellation:

```sh
cell-ci pause
cell-ci cancel JOB
cell-ci recover JOB
cell-ci resume
```

Pause prevents new jobs from being claimed and retains current work. Cancellation
requests a stop at the next safe boundary and drains active effects. It does
not undo an accepted commit or completed deployment. Failed jobs and cancellation
of active work pause new queue work. Cancelling a queued job prevents its
admission and sends no outcome email. Inspect an active failure before resuming.

Recovery reconciles the recorded operation through its owning provider. It does
not blindly repeat a merge, patch, model invocation, deployment, or email.
Unknown model execution, a lost Nucleus attempt, missing child completion
evidence, and unresolved deployment maintenance remain blocking conditions.
Process disappearance and elapsed time do not establish successful completion.

Resume refuses a blocked or unresolved active job. Service restart does not
clear that condition. Do not edit the journal, reset accepted history, delete
private refs, or invent a new provider identity to conceal an uncertain result.
There is no automatic journal migration, artifact cleanup, or unsupported-schema
recovery path.

Inspect the installed user service separately:

```sh
cell-ci service status
cell-ci service stop
cell-ci service start
```

The service runs `cell-ci worker` under singleton ownership. Do not start another
worker or alternate state directory to bypass its active job. The broker remains
the authority for individual gate admission and shared compiler resources.

## Coordinate Nucleus maintenance

Use an explicit owner for the manager's requester hold:

```sh
cell-ci maintenance hold --owner DEPLOYMENT_ID
cell-ci maintenance status
cell-ci maintenance release --owner DEPLOYMENT_ID
```

This is a Nucleus requester-admission hold. It blocks new repair submissions and
new queue claims and reports whether recorded model work has drained. Existing
non-model stages can finish. It does not suspend the manager's own deployment
handoff or take ownership of other providers' state.

Keep the hold until its recorded model operation is known to be drained. Release
only the exact owner acquired by the maintenance run. Hold release does not
resume an operator-paused queue, clear a blocked job, or remove another owner's
hold.

## Interpret deployment and notification

The manager freezes a deployment request ID, exact accepted source commit and
selected products. It reads that operation before starting or reconciling it.
The coordinator owns installation, maintenance release, and recovery. Matching
source and operation identity, successful installation evidence and released
maintenance establish the manager's deployment success. Cleanup failure can
remain visible even when installation and maintenance release succeeded.

The manager creates a deterministic outcome email after deployment or terminal
failure. It sends to Email's fixed personal recipient. The frozen message
contains the job, submitted/base/final commits, accepted state, repair count and
models, deployment outcome, and local artifact path. It does not attach source,
full CI transcripts, or model reasoning.

The manager saves the exact message and stable key before invoking Email. It
permits at most two Email invocations, at least five minutes apart and within
23 hours of notification creation. Email retains its own bounded internal
transport retries. Missing acceptance after the manager's limit marks the
notification uncertain and blocks the queue; it does not create a replacement
message or repeat deployment.

An Email acceptance receipt proves provider submission, not inbox delivery.
Notification state is separate from job, validation and deployment state. Read
the retained records when an email is absent; absence alone does not mean the
deployment failed.

## Protect retained state

On macOS, manager state is under
`~/Library/Application Support/Cell/ci-manager`. On other supported local
process hosts, the state path is `~/.local/state/cell/ci-manager`; macOS launchd
service installation is a separate platform requirement. State directories use
mode 0700 and private files use mode 0600. The journal is `queue.sqlite3` with
schema 1. Private worktrees, diagnostics and operation artifacts are below
`jobs/JOB/`.

The manager retains exact model requests and final patch responses, candidate
identities, CI logs and receipts, deployment correlations, and notification
payloads and receipts. It provides no automatic pruning. Protect these files as
private source and operational data. Provider retention remains separate.

Nucleus may transmit source and diagnostics read by the agent to its model
provider. Sending the outcome discloses its exact text, including local artifact
paths, to Resend and the fixed recipient's mail provider. The manager does not
copy Nucleus credentials or load Email's credential. Read-only inspection and
catalog discovery do not authorize these disclosures or start a job.
