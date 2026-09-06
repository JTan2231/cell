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

Each run creates a detached Git worktree at its selected commit. Pipeline drift
checks and brokered Cell recognition precede normal public product gates. A
complete product selection also runs the integrated Chancery source gate.
Public product `ci.sh --stage-candidate ABSOLUTE_DIRECTORY` executes the entire
existing gate and seals its exact executables **before releasing the broker's
heavy lane**. Later gates may reuse and overwrite the shared Cargo target; the
runner deploys the sealed copy, never a later view of `target/release`. Candidate
identity records the source commit, exact source key, executable hashes and
versions, and packaging/adapter source hashes. A staged directory is not admitted
without its exact passed broker receipt. Failed, lost, cancelled or stale gates
do not admit their staged bytes.

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
Capture terminal output externally if a deployment report is needed.
Failed preparation gates retain their CI transcripts under the separate broker
retention policy; the coordinator's own logs and workspace remain temporary.

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

An uncertain Nucleus apply retains its hold and requires the supported Nucleus
service recovery procedure. Matching files, declared versions, and health alone
cannot prove that an old resident daemon was replaced.

## Product adapter protocol, version 1

The product owns `PRODUCT_DIR/deployment/adapter.json` and `adapter.py`. The
metadata is literal JSON:

```json
{"schema":1,"product":"usher","dependencies":[],"description":"Install Usher"}
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
deployer, provider and adapter bytes. It is checked before every operation.

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
its publication lock, version policy, gate, commit, tag and atomic push. The
coordinator does not reinterpret an installed version as a release tag and does
not introduce a universal cadence. Changing cross-run CI evidence reuse or
adding automatic version/publication policy requires a separate reviewed
contract; neither is part of this version.

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
