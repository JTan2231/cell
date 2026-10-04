# Operate Telete

Telete is a separate Rust implementation of Cell CI orchestration. Source
installation does not replace or operate the existing CI manager. Telete uses
`refs/telete/accepted` and `refs/telete/jobs/` and does not advance development
`main` or `refs/ci/accepted`.

## Set up the host

Use the built Telete executable for first setup. Installation itself requires
configured storage, an initialized queue, and a usable signing identity.

```sh
telete storage configure --volume /Volumes/CellWork
telete storage status
telete signing create-local
telete signing status
```

Storage setup runs before queue paths are opened. It writes the shared
current-user `~/Library/Application Support/Cell/workspace.json`, with
`schema_version: 1`, the exact mount path and `volume_uuid`, and
`directory: "cell"`. The account database selects the home directory; `HOME`
and `--state` do not redirect this host file. The volume must be mounted,
external, writable APFS with ownership enabled. Setup creates a private
mode-0700 `cell` directory and publishes the mode-0600 selector without
replacing an existing file. Repeating an identical valid selection succeeds
without rewriting it. A different selection fails. Status reads the selection
and current volume observation without creating queue state.

Before first storage selection, pause and drain the legacy CI queue, release its
maintenance owners, stop its service, and settle its deployment. Rust setup
checks the service and reads its schema-one journal under the existing admission
and worker locks. It holds the legacy deployment lock and refuses retained
deployment recovery. It does not run Python, change the legacy journal, move
data, import job history, or select another accepted baseline. An unavailable
volume never selects a new queue. Restore the selected volume and its identity.

Initial signing creates the same shared Cell identity as Python setup. It writes
`~/Library/Application Support/Cell/signing.json`, not a Telete state override.
Creation requires configured storage but no initialized queue. It refuses an
existing shared or selected Telete policy, or an existing `Cell Local Signing`
certificate in the login Keychain. An invalid selection is not permission to
create a replacement.

The certificate uses RSA 3072, SHA-256, 3,650-day validity, and critical
`CA:FALSE`, digital-signature, and code-signing extensions. Setup imports the
certificate and key into the current user's login Keychain, authorizes
`/usr/bin/codesign` for key use, and adds user code-signing trust. Native user
authorization can be required. The SHA-1 fingerprint selects the exact
certificate; it is not the certificate signature algorithm. The schema-one
policy uses the `local` profile and `local.cell` namespace.

Shared signing writers hold the host setup lock, the selected Telete state's
admission, worker, and deployment locks, and the existing Cell queue admission
and deployment locks. Both queues must be paused and settled when present.
Stop the Telete worker before maintenance. Release publication and retained
deployment recovery must be settled. Settle any other independently configured
Telete states before changing a shared selection; these commands check the
selected state and the shared Cell queue, not an inventory of all consumers.
The commands do not pause, stop, cancel, or recover work for the operator.

Private generation files are removed on ordinary completion or failure. Only
Keychain retains the key. Setup preflights the identity before publishing the
policy. Keychain changes and file publication are not one transaction: a failure
or timeout can leave an imported certificate or key without configuration.
Inspect the reported fingerprint and Keychain effects. Do not delete or
regenerate the identity to repeat creation. Once that exact key is usable,
select it explicitly with:

```sh
telete signing configure --host --certificate-sha1 FINGERPRINT \
  --keychain /absolute/login.keychain-db --identifier-namespace local.cell
```

This command also supports deliberate shared identity changes under the same
maintenance guards. A failed preflight preserves the prior policy. If key
import did not complete, configuration alone cannot restore the missing key.
An abrupt process termination can also leave private generation staging for
explicit inspection and cleanup. No automatic renewal, identity replacement,
volume formatting, mounting, ownership change, or journal migration is supplied.

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

Run `telete worker` to keep the serial worker active. Use `telete pause` to prevent
new claims. Use `telete cancel JOB` for an intended stop at a safe boundary. Use
`telete recover JOB` to reconcile retained work. Use `telete wait JOB --timeout
60` to observe the same job without resubmission. Status is a CLI observation;
it is not an Iatreion probe or public CI client.

Reuse `--request-id KEY` only for the same frozen submission. A changed input,
test policy, deployment selection, repair policy, or notification policy needs
a new key. New jobs skip tests unless `--run-tests` is supplied. `--no-repair`,
`--no-deploy`, and `--no-notify` freeze explicit choices for that job.

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
own descriptor is `infrastructure/telete/product.sh`; it does not register Telete
with the existing CI inventory. Product, provider, package, and executable
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
production preparation. No existing Python CI helper or regression runner is
invoked. Receipts state when tests were skipped.

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
The matching provider directory selector follows the selected retained archive. Operational state stays external.
Telete does not install or replace `cell-ci` or its service.

Use `telete service status`, `service start`, and `service stop` for this service.
Service stop and program replacement require paused, drained work. Installation,
Semantics project registration, prompt import, queue resume, and live delivery
are separate operations. A source participation marker does not prove project
registration. No migration from existing CI journals is supplied.
