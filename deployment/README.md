# Cell deployment

Select systems from the Cell checkout:

```sh
./deploy.sh plan usher
./deploy.sh usher
./deploy.sh nucleus annals semantics krisis
./deploy.sh start nucleus annals semantics krisis
```

`plan` reads committed declarations and makes no runtime changes. Starting a
run selects the exact **local `main` commit** observed at admission; it ignores
uncommitted edits, other branches and later commits. It never fetches, changes
versions, commits, tags or pushes. Annals Usage is selected as part of the
Annals installation, and `decisions` is an alias for `krisis`. Dependency
declarations order selected products; each product's inspection proves required
installed dependencies rather than silently installing unselected products.

The invocation runs in the foreground until deployment finishes. By default it
prints one final JSON result to standard output. `--verbose` adds operation
progress on standard error; long-running operations otherwise produce at most
one liveness line per minute, starting after the first minute. Failure diagnostic
excerpts share a 4 KiB output budget across the original failure and recovery.
Each failure cause appears once in the final result, bounded to 1 KiB. Adapter
protocol replies and successful child logs are not terminal output. It uses no
model, Nucleus job, or conversation continuation.
Its Python executable and complete deployment source archive are pinned when
it starts. Python 3.11 or newer, Git, normal product build tools, and the current
macOS user session remain host prerequisites. The coordinator must be committed
on `main` before use. There is no background daemon or detached deployment API.

The deployment runtime uses Python's standard TOML reader for exact schedule
definition comparisons. Ordinary CI/broker bootstrapping and selector-only
script generation retain their existing Python 3.10 prerequisite.

## Preparation and cutover

Each run creates a detached Git worktree at its selected commit and calls the
shared release builder once for all selected products. Development is expected
to have completed the relevant CI checks. Deployment does not run CI or require
a prior CI receipt: preparation builds production binaries and checks their
versions, hashes, and exact source material. Tests, formatting, Clippy,
documentation builds, recognition gates, and generator CI remain development
checks.

The builder uses one release-profile Cargo invocation for the selected packages
and binaries, then seals independent product candidates in parallel. It keeps
a persistent target and file lock per logical Git repository, separate from
the CI broker and target. Cargo defaults to the logical CPU count capped at
eight; `CELL_RELEASE_BUILD_JOBS` accepts a positive override. Completed build
bundles persist in a content-addressed cache; preparation reuses a matching
bundle only after checking its exact material and executable integrity.

Build identity follows source bytes and build inputs rather than Git HEAD.
This lets publication build after updating versions and reuse those artifacts
when the same bytes become a commit. The existing schema-one candidate records
executable hashes and versions, and packaging/adapter source hashes. Deployment
binds that candidate to its exact selected commit after matching the source
material and retains a separate build receipt. The runner deploys sealed
executable copies, never a later view of a mutable Cargo target. A build record
makes no claim that CI passed.

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
2. Establishes every run-owned hold, then drains all affected products. An
   unselected requester may be held without changing its installed release.
3. Applies selected product adapters in their declared order.
4. Checks installed candidate identity and service readiness while every
   affected product remains held. No model jobs or synthetic records are created.
5. Releases requester holds only after all readiness checks pass, then releases
   Nucleus last.

The first stateful adapters deliberately use a conservative impact closure:
requester selection includes Nucleus, and Nucleus includes all its registered
requester products. Consequently a stateful run can hold and verify unselected
requesters, and all of those installations must already expose compatible
maintenance operations. It never upgrades them implicitly to satisfy that
precondition. Stateless selector-only pilots do not require this bootstrap.

The coordinator owns sequencing and temporary execution state. Product adapters own
configuration discovery, admission, quiescence, migration, service control,
runtime readiness and recovery. Existing deployers retain their ownership,
product and Chancery writer locks. The shared generated profile below stays
limited to products whose deployment only changes program/documentation
selectors. The coordinator does not turn those scripts into stateful deployers.

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

Success means all required readiness checks passed and all run-owned holds were
released. A completed failure uses the existing product recovery operations
within that invocation. Recovery must identify a coherent prior or candidate
installation and prove that releasing each hold is safe; every proof precedes
release, and Nucleus releases last. Recovery never repeats an uncertain apply
or clears another owner's hold. A failed deployment still returns failure even
when recovery succeeds. Unsafe recovery retains the product's holds and reports
the owner and error for its supported operational recovery procedure.

After the worker exits, the parent closes its inherited lock descriptor and
reacquires the host lock before unregistering the worktree and removing all
temporary run files. A surviving descendant blocks this cleanup. The next
deployment removes stale inactive workspace only after obtaining that same lock;
it does not resume or recover an interrupted deployment. Both original failure
and recovery evidence are collected before cleanup and emitted once afterward.
Capture terminal output externally if a deployment report is needed. The
coordinator's logs and workspace remain temporary. Completed build bundles and
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

Product command failures retain a short structured error code or a recognized
operational failure category. The shared adapter does not relay arbitrary child
messages, command arguments, credentials, or domain bodies; unrecognized failures
retain their executable and exit status.

After successful readiness and release, the coordinator removes unreferenced
installed Cell release history under the same global lock. It preserves current
releases and releases pinned by selected schedules (including disabled ones),
service definitions, current product receipts/configuration, and running
processes. Cleanup errors are reported separately: installed products stay in
place and no recovery or rollback begins. Direct legacy product deployers still
retain their own previous releases; automatic pruning belongs to a successful
coordinated deployment.

The cleanup reader accepts legacy complete Clockwork binding arrays and expands
version-two selection pages until the inventory is complete. Unknown or incomplete
inventories stop cleanup before deletion.

An uncertain Nucleus apply retains its hold and requires the supported Nucleus
service recovery procedure. Matching files, declared versions, and health alone
cannot prove that an old resident daemon was replaced.

## Product adapter protocol, version 1

The product owns `PRODUCT_DIR/deployment/adapter.json` and its adapter
implementation. Most products use `adapter.py`; Usher declares the sealed
installer executable in literal JSON:

```json
{"schema":1,"product":"usher","dependencies":[],"application":"Usher","adapter_binary":"usher-install","description":"Install Usher"}
```

`dependencies` contains ordering prerequisites among selected systems. It is not
a claim that Chancery dependency edges describe deployment order. An adapter is
invoked with a fixed operation argument: `inspect`, `hold`, `drain`, `apply`,
`verify`, `release`, or `recover`. No caller-supplied command or workflow body is
accepted. Standard input is one JSON object containing:

- `schema`, `product`, `run_id`, `run_dir` and immutable `source_root`;
- `selected_products`, `candidate_dir`, and the candidate manifest;
- `prior`, the opaque data captured by that product's inspection;
- `recovery`, the captured operation and hold/application/verification progress
  during recovery within the active invocation.

`candidate_dir` and `candidate` are null for affected-only products. Binaries
reside at `candidate_dir/bin/COMMAND`; `candidate.binaries[COMMAND]` records
`path`, `sha256` and `version`. The source archive contains version-matched
deployer, provider and adapter source bytes. It is checked before every operation.
For Usher, candidate sealing includes `usher` and `usher-install`; its adapter
executes `candidate_dir/bin/usher-install adapter OP` rather than compiling or
using the shared target during cutover. The enclosing coordinator and other
products' adapters retain their existing Python implementation.

Standard output must be exactly one bounded JSON object, with diagnostics on
standard error:

```json
{"schema":1,"status":"ready","detail":"Owned installation inspected","data":{}}
```

The expected statuses, respectively, are `ready`, `held`, `drained`, `applied`,
`verified`, `released` and `recovered`. Nonzero exits, `stopped`, invalid replies,
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
Successful recovery additionally returns
`{"safe_to_release":true,"installed":"candidate"}` or `"installed":"prior"`
inside `data`. Missing proof retains maintenance. Recovering a product is never
permission to bypass its ownership, migration or forward-only authentication
boundary.

## Release publication policy

Deployment consumes committed declared versions and content identities. Git
release publication remains the separate product `release.sh` operation with
its publication lock, version policy, release build, commit, tag and atomic
push. The coordinator does not reinterpret an installed version as a release
tag and does not introduce a universal cadence. Release and deployment share reusable build
artifacts; they do not reuse or enforce completed CI evidence.

## Usher's Rust installer

Usher's separate `usher-install` executable uses the shared `cell-install`
library for local installation mechanics. It owns Usher's product policy and
the version-one coordinator adapter; the `usher` recognition command remains
read-only. `cell-install` is workspace infrastructure with no separate product
identity or release publication. Only Usher uses this installer path.

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

Usher no longer has a checked-in shell installer or Python product adapter.
Conversations, Geste and CRM retain the generated profile below, and stateful
products retain their product-owned lifecycle and recovery mechanics.

## Selector-only deployment generation

`generate.py` renders complete, self-contained macOS deployers for products
whose deployment changes only an immutable program release and its command and
Chancery provider selectors. Product runtime state is outside this profile.
Generation requires Python 3.10 or newer; generated deployers themselves use
only the documented macOS shell tools.

Regenerate or check the checked-in scripts with:

```sh
python3 deployment/generate.py
python3 deployment/generate.py --check
```

The profile has no arbitrary shell hooks. A product that needs service control,
database work, maintenance, scheduling, or authentication keeps
a product-owned deployer. Every product that publishes a Chancery provider uses
the same catalog-writer lock; custom/stateful profiles also keep their existing
product and lifecycle locks and are conservatively declared as globally
conflicting to an orchestrator. The generated deployer stages outside the
shared catalog lock, takes its product lock before the catalog writer lock,
publishes one atomic `current` selector, and packages its exact own bytes for
rollback.

Each descriptor supplies only product identity/display names, application
support/binary/provider names, help text, the existing manifest-format toggle,
and existing completion-output variants. There are no lifecycle hooks.

`--expected-current absent|releases/<sha256>` supplies an optional optimistic
concurrency precondition. When omitted, the deployer snapshots `current` before
waiting for its product lock and rejects the operation if that selection changes.
