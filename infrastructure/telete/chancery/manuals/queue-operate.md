# Operate Telete

Telete is Cell's Rust CI system. Root and product `ci.sh` wrappers select the
installed `telete` command. Telete uses `refs/telete/accepted` and
`refs/telete/jobs/`. It does not advance development `main`.

## Set up the host

Use the built Telete executable for first setup. Installation requires configured
storage, an initialized queue, and a usable signing identity:

```sh
telete storage configure --volume /Volumes/CellWork
telete storage status
telete signing create-local
telete signing status
```

The selected volume must be mounted, external, writable APFS with ownership
enabled. Storage publication is create-only. An unavailable volume never
selects another queue. Restore the selected volume and identity.

Signing uses the shared host policy unless the selected Telete state has an
explicit override. Keep the exact selected certificate and Keychain identity.
Before changing signing, pause and drain the queue, stop the worker, and settle
release publication and Telete and manual deployment recovery. Configuration
does not do those
operations for the operator. Failed preflight preserves the policy; interrupted
initial creation can leave Keychain effects that require explicit inspection
and selection of that exact identity.

Read `telete.signing.operate` for the complete shared storage and signing promise,
configuration format, initial creation, identity selection, and rotation limits.

## Initialize and control a queue

Select state below the configured external Cell workspace. The default is its
`telete` directory. An explicit directory must begin with `telete` or `telete-`.
Telete verifies the mounted volume identity, external APFS ownership, and private
directory permissions. It supplies external temporary, Cargo, target, and cache
paths. Compiler and check commands use the macOS filesystem sandbox.

Initialize with an exact acceptable baseline. Initialization does not validate
that baseline and starts with admission paused:

```sh
telete init --repo /absolute/cell --accepted-baseline COMMIT
telete status
telete resume
telete submit COMMIT --repo /absolute/cell --run-tests
telete worker --once
```

After installation, run `telete service start` to start the installed worker.
Run `telete worker` for a directly operated worker. Use `telete pause` to prevent
new claims. Use `telete cancel JOB` for an intended stop at a safe boundary. Use
`telete recover JOB` to reconcile retained work. Use `telete wait JOB --timeout
60` to observe the same job without resubmission. Status is a CLI observation;
it is not an Iatreion probe or public CI client.

Use `telete cancel JOB` to abandon one paused blocked validation after its
bounded dispatcher timed out without an aggregate report. The command requires
the exact timed-out dispatcher receipt, terminal child evidence, exclusive
compiler and deployment resources, unchanged accepted base and private source,
an accepted failure notice, and no other queued work, preparation, acceptance,
deployment, or unresolved model work. It records the original operation in
`validation-N.abandoned.json`, clears its unresolved intent, and sets the job
phase to `cancelled`. The original failed outcome, notification, and process
records remain retained. Cancellation does not replay validation or claim a
pass. Submit the intended source with a new request ID for another delivery job.

Reuse `--request-id KEY` only for the same frozen submission. A changed input,
test policy, deployment selection, repair policy, or notification policy needs
a new key. New jobs skip tests unless `--run-tests` is supplied. `--no-repair`,
`--no-deploy`, and `--no-notify` freeze explicit choices for that job.

New jobs with notifications require the public Email command to resolve to the
regular file at `~/Library/Application Support/Email/install/runtime/bin/email`.
Telete saves that fixed path in the job policy. Email updates replace the file
at that path; the policy does not freeze Email release bytes. Install Email with
fixed runtime support before submitting these jobs. Existing jobs retain their
saved command paths, including archive paths. Reusing an existing request ID
returns its original job without selecting a new Email path.

Use `telete maintenance hold --owner OWNER`, `maintenance status`, and
`maintenance release --owner OWNER` to coordinate requester maintenance. Release
only the owner acquired by that operation. A release does not clear operator
pause or an unresolved job.

## Validate source

Each claimed job captures Telete's accepted source as its fixed base and merges
the submitted commit in a private worktree. Conflicts stop the job. Repairs
produce new private candidates. Validation compares each candidate to the same
base, including deletions and both rename paths.

The validator loads literal product descriptors and Cargo metadata. Telete's
own descriptor is `infrastructure/telete/product.sh`. Other product descriptors
remain in `pipeline/products/`. Product, provider, package, and executable
identities remain separate.

Product changes select their owner. Explicit prompt and shared platform inputs
also select their consumers. Changes to `cell-install` or `cell-maintenance`
select their direct local Cargo consumers. Common Telete validation inputs
select the full product and platform inventory. Root `Cargo.toml` and
`Cargo.lock` changes do not select all products. Other local Cargo dependencies
do not expand product selection.

With tests enabled, each selected platform product adds the shared
`cell-install` tests. A platform product with a direct local `cell-maintenance`
dependency also adds that suite. Adding these tests does not select more
products. New product introductions follow the same rules.

Default deployment uses the selected products, except Telete. An explicit
`--deploy PRODUCT` list selects deployment products, and `--no-deploy` prevents
deployment. Test selection does not add products to an explicit deployment list.

Contract 2 narrows product selection and adds the shared platform tests above.
Validation receipts remain schema 1. Retained receipts keep their recorded
scope; this change does not migrate or reinterpret them.

The host compiles the exact candidate's Telete validator in an isolated target
directory. Candidate code owns validation. Host Telete code owns promotion,
production preparation, and signing. Missing candidate support fails explicitly.
The validator runs native structure, provider, shell syntax, formatting, Clippy,
selected nextest, and release checks. Deferred release checks run in trusted
production preparation. Receipts state when tests were skipped.

The candidate validator dispatcher has no whole-validation deadline. Individual
commands retain their positive time limits. Unbounded dispatchers use distinct
gate identities; retained bounded dispatchers keep their recorded requests and
deadlines. Process completion and the aggregate validation report remain separate
requirements for validation success.

Install the pinned nextest runner explicitly with `telete prepare-tools` before
jobs that run tests. Validation does not download a missing runner.

Machine-applicable Clippy suggestions and formatting can produce one raw Git
patch from an isolated snapshot. The manager records a new private candidate
and validates it again. The original candidate remains unchanged.

The broker admits one heavy gate and two light gates. Equal gate identities
join the retained result only for the same command. Different commands cannot
reuse an identity. Missing terminal evidence blocks the resource slot; process
disappearance is not success. `telete gates` exposes retained diagnostic records.
No automatic pruning or uncertain-gate replay is supplied.

## Repair through providers

Repair uses `nucleus-core` and `nucleus-client`. Nucleus owns execution and its
credential. Telete freezes the exact request, requester identity, model policy,
and dedicated Bazaar prompt versions before admission. Read-only workspace
access, local execution, and disabled web search are explicit request policy.

Bazaar must contain `cell.prompts.telete`, whose schema-one `entries` map pins
positive versions of `telete.repair.instructions` and `telete.repair.prompt`.
The prompt contains `{context}`. Read the source seed at
`../../prompts/seed.json`; import its reviewed components before its selection.
Runtime reads do not initialize Bazaar or fall back to source prompt files.

Only the final model response supplies the raw patch. Git decides whether it
applies to the private parent index. Telete records the resulting candidate
before refunding that attempt's budget point. Rejected or failed attempts stay
charged. Refunded attempts retain unique history and can exceed the total
charged budget's invocation count.

Quota deferral and uncertain admission preserve the same request. Recovery
observes the exact Nucleus identity. An authoritative `not_found` permits that
same request; lost or unavailable execution remains blocked. A repair proposal
does not prove validation or source acceptance.

## Prepare and execute deployment

Telete builds selected production packages together, stages declared binaries,
and signs native commands with the frozen certificate and
stable product identifiers. Telete reads the shared host signing selection by
default. `telete signing configure --certificate-sha1 FINGERPRINT --keychain
/absolute/keychain --identifier-namespace local.cell` selects an existing
certificate in Telete's own configuration. Add `--host` to change the shared
Cell policy. Initial `signing create-local` also selects the shared policy;
an existing Telete override retains precedence. Configuration and installation require
paused, drained Telete work and settled Telete deployment.

Use `telete signing status` to inspect the selected identity. Keychain access or
native authorization can require the user. Signing and preparation failures
stop before source acceptance. Compiler failures can use the frozen repair
budget. There is no alternate certificate or unsigned fallback.

Inspect retained gate receipts and staging effects when preparation stops without
a final receipt. Stop the worker and pause the queue. Use `telete
acknowledge-preparation JOB` only to abandon that blocked preparation. The command
requires exclusive worker and resource ownership, exact terminal child receipts,
unchanged accepted source, and no source acceptance, deployment, or unresolved
model work. It retains the original operation in an acknowledgement record,
preserves the failed outcome and its settled notification, and cleans the owned
worktree. It does not replay preparation, publish staged candidates, or report
preparation success. A repeated acknowledgement joins the same record. Submit a
new commit with a new request ID after correcting the failure.

Promotion checks runtime source, scope, and policy correlation and uses an
expected-old reference update. Telete retains the facts in its journal.

Deployment executes exactly the selected product manifests' `run`, `copy`, and
`link` instructions in order. Product installers read package assets from the
exact source worktree. Product installation commands own program
selection, state, configuration, schedules, and service lifecycle. A completed
instruction does not establish product health or domain success. A failed
instruction retains earlier effects. An interrupted instruction is uncertain
and is not automatically repeated or rolled back.

Inspect the retained deployment before using `telete acknowledge-deployment
REQUEST`. Acknowledgement requires exclusive deployment ownership, records the
uncertainty, and releases the interrupted operation. It does not undo effects,
run remaining instructions, or establish product health. Then use `telete
recover JOB` to reconcile the job with that retained outcome.

Accepted source remains accepted after deployment failure. Notification is a
separate stage. Telete freezes deterministic outcome text and an Email key
before sending through the supported Email client. Email acceptance means
provider submission, not final inbox delivery. Notification failure does not
restart deployment. Private requests, source, diagnostics, and receipts remain
retained; settled worktree cleanup does not remove their records.

## Install only on explicit request

Build Telete and invoke `telete install` only when installation is intended. The
queue must be initialized, paused, and settled first. The
installer publishes the executing binary and matching embedded provider in its
own retained release archive. It publishes regular executable files at the fixed
`~/Library/Application Support/Telete/install/runtime/bin/telete` path. Both the
public `~/.local/bin/telete` command and the `org.cell.telete` user service use
that runtime file. Updates preserve its actual executable path. Each runtime
file replacement is atomic; the complete tree is not one atomic update.
The matching provider directory selector follows the selected retained archive.
Operational state stays external.

Installation leaves the Telete service stopped. Start it explicitly, then
resume queue admission.

Use `telete service status`, `service start`, and `service stop` for this service.
Service stop requires paused, drained work. It also permits one blocked deployment
whose exact successful receipt matches its unchanged accepted source and product
scope, with settled notification and no queued work, unresolved model, active
operation, or live compiler or deployment child. The stop holds exclusive settled
resource ownership and preserves the job and receipts for `telete recover JOB`;
it does not repeat deployment. Program
replacement and signing maintenance still require paused, drained work. Installation,
Semantics project registration, prompt import, queue resume, and live delivery
are separate operations. A source participation marker does not prove project
registration. No cross-release journal migration window is supplied.
