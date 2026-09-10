# Cell deployment

Select systems from the Cell checkout:

```sh
./deploy.sh plan usher
./deploy.sh usher
./deploy.sh nucleus annals semantics krisis
./deploy.sh start nucleus annals semantics krisis
```

`plan` reads committed declarations without runtime changes. At admission, a
run selects the exact **local `main` commit**. It ignores uncommitted edits,
other branches, and later commits. It never fetches, changes versions, commits,
tags, or pushes. Selecting Annals also selects Annals Usage. `decisions` is an
alias for `krisis`. Dependency declarations set the installation order. The plan
adds missing dependencies and dependencies outside the consumer's declared
`runtime_versions` interval. An unproved installed version selects the committed
dependency candidate. Incompatible committed candidates stop before maintenance.
Installed companions receive matching candidates, including consumers that embed
the provider's Rust libraries. The plan reports each addition's reason.

Retained dependencies receive a read-only inspection from their sealed owning
installer before maintenance and after configuration. Installation remnants,
including broken selectors, are inspected rather than treated as absence.
Version compatibility does not establish product readiness; adapters also check
their supported runtime interfaces and configuration.
Consumers can also require an explicitly indexed installed interface contract.
For example, Mentor and EMT require Email's account setup and discovery operation.
An older Email release with the same release number but without that operation
is selected for replacement. These explicit requirements do not turn Chancery's
documentation dependency graph into runtime or deployment edges.

Supply initial choices through a private JSON file keyed by canonical product
name. Subsequent runs reuse product configuration:

```sh
./deploy.sh mentor --settings /absolute/setup.json
```

For example, `{"mentor":{"receiving_domain":"reply.example.com","enabled":false}}`
prepares Mentor without enabling its worker. Product installation manuals define
their accepted keys. Supply credential file references, never credential bytes.
Inspection gathers missing choices before maintenance. Email owns credential
installation and receiving-account discovery. A `plan` lists settings owners
without printing their values.

The command runs in the foreground until deployment ends. By default, it prints
one final JSON result to standard output. `--verbose` adds progress on standard
error. Otherwise, operations report activity at most once per minute after the
first minute. Failure and recovery excerpts share a 4 KiB output limit. Each
failure cause appears once in the result, with a 1 KiB limit. Adapter replies
and successful child logs are not printed. Deployment uses no model, Nucleus
job, or conversation continuation.

At startup, deployment pins its Python executable and complete source archive.
The host needs Python 3.11 or newer, Git, product build tools, and the current
macOS user session. Commit the coordinator on `main` before use. There is no
background daemon or detached deployment API.

The coordinator and release builder use Python. Product installation and
deployment adapters are Rust executables backed by `cell-install`; retained
shell frontends are runtime assets for credential loading and scheduled jobs.

## Preparation and cutover

Each run creates a detached worktree at the selected commit. It calls the shared
release builder once for selected products, the maintenance closure, and retained
dependencies. Preparing an inspector does not select its product for upgrade.

Complete the relevant CI checks during development. Deployment does not run CI
or require a CI receipt. Preparation builds production binaries and checks
versions, hashes, and source material. Tests, formatting, Clippy, documentation
builds, recognition gates, and generator CI remain development checks.

The builder uses one release-profile Cargo invocation for the selected packages
and binaries, then seals independent product candidates in parallel. It keeps
a persistent target and file lock per logical Git repository, separate from
the CI broker and target. Cargo defaults to the logical CPU count capped at
eight; `CELL_RELEASE_BUILD_JOBS` accepts a positive override. Completed build
bundles persist in a content-addressed cache; preparation reuses a matching
bundle only after checking its exact material and executable integrity.

Source bytes and build inputs determine build identity. Git HEAD does not.
Publication can therefore build updated versions and reuse those artifacts
after the same bytes are committed. A schema-one candidate records executable
hashes and versions, plus packaging and adapter source hashes. Deployment
matches the source material, binds the candidate to its selected commit, and
retains a separate build receipt. It deploys sealed executable copies without
reading a later Cargo target. Build records do not record CI success.

The same builder can prepare candidates without publication or installation:

```sh
python3 deployment/build.py --source-root /absolute/cell \
  --product usher --product nucleus --output /absolute/cell-build
```

The output contains `candidates/PRODUCT/bin`, each product's `candidate.json`,
and `result.json`. With a single product, optional `--unit UNIT` selects one
independently versioned release unit for release preparation. The existing
product `ci.sh --stage-candidate ABSOLUTE_DIRECTORY` interface still runs the complete
CI gate for callers that explicitly choose it.

All candidates are prepared before maintenance begins. The coordinator then:

1. Inspects selected products and every additional product whose admission must
   be held. It retains each product-owned baseline, prerequisites and ordering.
2. Holds and drains each consumer before its providers, with Nucleus last, so
   admitted work can finish using its dependencies. An unselected requester may be held
   without changing its installed release.
3. Applies selected product adapters in their declared order, then configures
   affected products in dependency order. Products with atomic state-and-file
   transactions stage releases during apply and commit that transaction during
   configure. Nucleus starts its replacement service under its existing hold so
   requester configuration can use it.
4. Checks installed candidate identity and service readiness while every
   affected product remains held. No model jobs or synthetic records are created.
5. Releases requester holds only after all readiness checks pass, then releases
   Nucleus last. Activation follows release of all holds.

Replacing Nucleus holds its installed requesters, as declared by each product's
`requester_service`. Replacing one requester leaves Nucleus admission open.
Affected-only installations must provide compatible maintenance operations.
Mentor and EMT capture worker intent during inspection, suspend their bindings,
select disabled definitions during configuration, and restore enabled state
after verification and hold release. Existing pauses and incidents remain.
An uninitialized Mentor installation uses its product default activation policy.
Mentor and EMT resolve receiving configuration before maintenance. Conatus
preserves library identities and its cursor while rebinding Annals. Paperboy and
Platter reselect existing schedules with the installed program's exact pins.
Clockwork suspends every captured enabled binding. During activation, product
adapters restore their declared exact keys; Clockwork restores other captured
bindings through its current broker. Custom bindings and incident halts survive.

The coordinator owns the operation sequence and temporary execution state.
Product adapters own configuration discovery, admission, quiescence, migration,
service control, readiness, and recovery. The shared transaction library owns
immutable artifact manifests, public file selection, writer locks, attribution
checks, and file compensation.

Products own runtime state, admission holds, database backups, schedules,
services, and recovery decisions. Prove database recovery before restoring public
commands. Nucleus's guarded service installer owns copied executables and
authentication state.

## Temporary state and failure handling

Private active state is under
`~/Library/Application Support/Cell/deployments/active` on macOS. While running,
it contains operation inputs/outcomes, logs, sealed source, executable candidates,
and the detached preparation worktree. These files may contain private paths
and baseline state; adapters must exclude credentials and domain document bodies.
There is no retained deployment history or public status, wait, resume, or
recover interface.

The foreground process acquires one host-wide deployment lock before creating
that workspace and passes its descriptor to the pinned worker. Every adapter
and deployer inherits the same lock. A surviving child therefore keeps another
deployment out even if its supervisors exit. Existing product locks and
admission gates remain necessary for coexistence with direct commands.

Success requires all readiness checks to pass and all run-owned holds to be
released. After a completed failure, the same invocation uses product recovery
operations. Recovery must establish a coherent prior or candidate installation
before releasing holds. Each release requires proof that it is safe; Nucleus
releases last. Recovery never repeats an uncertain apply or clears another
owner's hold. Successful recovery does not change a failed deployment result.
If recovery cannot safely release a hold, it retains that hold and reports its
owner and error for product recovery.

If any release or activation was attempted, recovery first holds and drains the
affected products again. It preserves the original inspection baseline. Product
recovery then repairs the transaction and configuration before a new release and
activation attempt. A lost activation reply does not authorize configuration
changes while work is running.

After the worker exits, the parent reacquires the host lock. A surviving
descendant blocks cleanup. A resolved transaction is removed. An unresolved
transaction remains private with its source, candidates, baselines and logs.
The next ordinary deployment first invokes recovery using that retained
transaction. It starts the requested deployment only after recovery succeeds.
No caller-supplied run ID or separate recovery command is required. A failed
recovery retains evidence and holds. Original failure and recovery excerpts
share the final diagnostic budget.

Completed build bundles and
the release Cargo target live outside that workspace and survive cleanup. The
default cache is `cell-release-cache` under the repository's Git common
directory, shared by linked worktrees; `CELL_RELEASE_CACHE_DIR` overrides it.
The build cache has no automatic pruning.

The final result preserves `schema`, `run_id`, `state`, `products`,
`source_commit`, `detail` and `exit_code`. It additionally reports `recovery`
(`not_needed`, `succeeded`, `failed`, or an interrupted outcome), `maintenance`
(`not_started`, `released`, or `attention_required`) and `cleanup` dispositions
for installed releases and the temporary workspace. Outstanding maintenance
lists its owner and each affected product as `retained` or `uncertain`. A lost
hold or release reply is uncertain until a successful release is captured.
Successful recovery still returns deployment failure, with explicit released
maintenance. Cleanup failure preserves the verified installation outcome.

Product command failures retain bounded execution status. The shared adapter
does not relay arbitrary child messages, arguments, credentials, or domain
bodies. Unrecognized failures retain the executable and exit status.

After successful readiness and release, the coordinator removes unreferenced
installed Cell release history under the same global lock. It preserves current
releases and releases pinned by selected schedules (including disabled ones),
service definitions, current product receipts/configuration, and running
processes. Cleanup errors are reported separately: installed products stay in
place and no recovery or rollback begins. Direct product installers
retain their own previous releases; automatic pruning belongs to a successful
coordinated deployment.

The cleanup reader accepts complete legacy Clockwork binding arrays. For
version-two selections, it reads pages until the inventory is complete. An
unknown or incomplete inventory stops cleanup before deletion.

Nucleus retains its cutover journal and uses its service-owned recovery procedure
to establish the resident generation. Matching files, declared versions, and
health alone cannot prove that an old resident daemon was replaced. An unproved
cutover retains the hold and recovery evidence.

## Product adapter protocol, version 1

The product owns `PRODUCT_DIR/deployment/adapter.json` and its Rust installer.
Each declaration names its sealed installer executable in literal JSON:

```json
{"schema":1,"product":"usher","dependencies":[],"application":"Usher","adapter_binary":"usher-install","description":"Install Usher"}
```

`dependencies` declares installation prerequisites, separately from Chancery
documentation dependencies. `runtime_versions` maps each dependency to inclusive
`minimum` and exclusive `before` release versions. Maintain these bounds with the
product's supported interfaces. Optional `runtime_contracts` maps a dependency
to required installed entry IDs and inclusive `minimum`/exclusive `before`
contract versions. Only explicitly indexed, supported entries satisfy it.
`companions` selects installed consumers that need matching artifacts or pins.
`requester_service` names the service whose
replacement requires this installed requester to drain. `activation_bindings`
lists the exact Clockwork keys whose final intent the adapter owns. Optional
`pin_inventory` names read-only product CLI arguments that return its complete
configured executable references. Build, deployment and cleanup read the same
literal product inventory. The adapter accepts one fixed operation argument:
`inspect`, `hold`, `drain`, `apply`, `configure`, `verify`, `release`, `activate`,
or `recover`. It accepts
no caller-supplied command or workflow body. Standard input is one JSON object:

- `schema`, `product`, `run_id`, `run_dir` and immutable `source_root`;
- `selected_products`, `candidate_dir`, and the candidate manifest;
- `affected_products` and their exact `activation_bindings`;
- optional product `settings`, direct `dependency_settings`, and sealed
  `dependency_candidates` for read-only discovery before a first installation;
- `prior`, the opaque data captured by that product's inspection;
- `recovery`, the captured operation and hold/application/verification progress
  during recovery within the active invocation.

`candidate_dir` and `candidate` identify a sealed candidate for selected and
affected products. Binaries reside at `candidate_dir/bin/COMMAND`; the manifest
records each path, SHA-256 and version plus exact packaging/provider source
hashes. The adapter executes `PRODUCT-install adapter OP` from the candidate,
checks the source and its own bytes, and refuses `apply` for an affected-only
product. A declared `maintenance_products` closure makes these executables
available before inspection and before any hold.

Standard output must be exactly one bounded JSON object, with diagnostics on
standard error:

```json
{"schema":1,"status":"ready","detail":"Owned installation inspected","data":{}}
```

The expected statuses, respectively, are `ready`, `held`, `drained`, `applied`,
`configured`, `verified`, `released`, `activated` and `recovered`. A drain may
return `waiting`; the coordinator waits and retries that phase in the same run.
Waiting has no deployment-duration cutoff. Nonzero exits, `stopped`, invalid replies,
unknown statuses and outputs above 1 MiB stop the run. `inspect` data may declare
`maintenance_products` and `after` as lists of canonical system names.
Inspection must be read-only with respect to the installation.

Every hold is owned by `run_id`. Hold and release must be idempotent, preserve
pre-existing operator pauses and other runs' holds, and support an interrupted
response. Recovery releases every affected run-owned token idempotently after
proof, including when a hold's effect happened but its reply was lost. Each
mutation must retain enough product-owned evidence for recovery. The coordinator
records `any_apply_started` before permitting any product apply; an unchanged
prior installation may recover pre-cutover holds using its inspected baseline.
Recovery context also records configure, release and activation progress,
including attempts whose reply was lost. Each product determines its actual
cutover from its own transaction evidence.
Successful recovery additionally returns
`{"safe_to_release":true,"installed":"candidate"}` or `"installed":"prior"`
inside `data`. Missing proof retains maintenance. Recovering a product is never
permission to bypass its ownership, migration or forward-only authentication
boundary.

## Release publication policy

Deployment uses committed versions and content identities. Product `release.sh`
separately owns Git publication: its lock, version policy, build, commit, tag,
and atomic push. The coordinator does not treat an installed version as a Git
tag or impose a release cadence. Release and deployment share build artifacts.
They neither reuse nor enforce completed CI results.

## Usher's Rust installer

Usher's separate `usher-install` executable uses the shared `cell-install`
library for local installation mechanics. It owns Usher's product policy and
the version-one coordinator adapter; the `usher` recognition command remains
read-only. `cell-install` is workspace infrastructure with no separate product
identity or release publication. Usher retains its compatible version-one file format. The other products use
the version-two transaction format described below.

Prepare both executables with the shared release builder, then install them:

```sh
python3 /absolute/cell/deployment/build.py --source-root /absolute/cell \
  --product usher --output /absolute/cell-build
/absolute/cell-build/candidates/usher/bin/usher-install install \
  --binary /absolute/cell-build/candidates/usher/bin/usher \
  --bundle /absolute/cell/usher/chancery
```

The installer verifies version alignment, content integrity and ownership,
takes the product lock before the Chancery writer lock, and publishes one
atomic `current` selector. It supports an exact `--expected-current
absent|releases/HASH` precondition and intentional `--home ABSOLUTE_PATH`.
The immutable release contains `bin/usher`, `bin/usher-install`, the exact
recovery executable at `package/install`, and `share/chancery/usher`. Its
`manifest.json` uses format `cell-install-v1`, records product/provider versions
and each retained file's SHA-256 and mode, and identifies the complete release
by content. Both public command selectors and the provider selector follow
that release.

`inspect` reports owned installed state. `verify --binary ABSOLUTE_PATH
--bundle ABSOLUTE_PATH` proves the installed exact candidate using the executing
installer; `verify-release ABSOLUTE_RELEASE_DIR` checks retained release
integrity without changing selectors. `recover --release ABSOLUTE_RELEASE_DIR`
restores a supported retained release under the same ownership and expected
selection checks. The retained Rust `package/install` also reads and restores
the supported legacy `manifest.txt` shell release format. Legacy recovery
detaches the owned public `usher-install` selector; inspection accepts its
absence while that release is current. Keep the retained Rust recovery
executable to reselect its release later. The legacy `package/deploy-user.sh`
is archived release evidence and cannot handle a new-format current release.
Recovery does not edit immutable release bytes.
See [Usher installation](../usher/chancery/manuals/install-operate.md) for
the exact operational boundary.

## Shared Rust installation transactions

Every product builds a dedicated `PRODUCT-install` executable alongside its
runtime executables in the release builder's single Cargo invocation. Product
versions remain independent; Annals Usage remains a separate release unit.
The `cell-install-v2` manifest records exact file digests and modes, independent
executable/provider versions, public entry mappings and a stable content identity.
The immutable tree retains `package/install` for supported recovery. Legacy
formats are accepted only through the product's explicit complete byte proof.

Conversations, CRM, Chancery, Email, Cast, Clockwork and Platter use the common
program-selection entry point. Their product specifications supply the layout,
legacy proof and runtime frontend where needed. Clockwork additionally validates
its provider with the supplied Chancery reader; its runtime recognizes its own
fully verified version-two release when pinning schedule definitions. CRM's
coordinated route preserves admission and explicit database migration handling.
Platter requires the coordinated route for every mutating installation or
recovery operation. Its shared per-user admission gate includes custom runtime
state, and drain observes both Platter and predecessor Job Packets Nucleus
identities. The initial release preserves schema 1 and existing artifact paths,
checks dependencies and backs up the database without creating domain work.
Its installer creates no daily delivery schedule.
Stateful products provide typed lifecycle code around the same file transaction.
See each installed product's installation contract for its exact arguments and
recovery limits.

Publication rechecks the captured selection under the product lock and holds
the Chancery writer lock through validation and compensation. Suspended public
commands stay absent during state rollback. Explicit interrupted-publication
recovery accepts only absent entries or entries attributable to the captured
prior release and exact candidate; foreign replacements are retained and stop
recovery. Directory locks preserve the legacy mkdir protocol and reclaim only a
recognized private owner marker whose process is proven dead. Empty legacy locks
and unknown or live owners require product/operator recovery.

After coordinated success, cleanup uses the supplied sealed candidate installers
for read-only `verify-release` checks. If a product has no such verifier, cleanup
retains all its history. It completes every live-reference, receipt,
transaction-marker, and release check before deletion. Current releases and
selected schedule pins, including disabled bindings, remain protected. Cleanup
never uses retained release executables as its authority.

Platter participates in this release-history cleanup through its sealed
installer and PID-aware file lock. Its private packet state and database
backups remain outside the installation tree and are retained.
