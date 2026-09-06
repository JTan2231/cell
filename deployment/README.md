# Cell deployment

Select systems from the Cell checkout:

```sh
./deploy.sh plan usher
./deploy.sh usher
./deploy.sh nucleus annals semantics krisis
./deploy.sh status RUN_ID
./deploy.sh wait RUN_ID --timeout 30
./deploy.sh resume RUN_ID
./deploy.sh recover RUN_ID
```

`plan` reads committed declarations and makes no runtime changes. Starting a
run selects the exact **local `main` commit** observed at admission; it ignores
uncommitted edits, other branches and later commits. It never fetches, changes
versions, commits, tags or pushes. Annals Usage is selected as part of the
Annals installation, and `decisions` is an alias for `krisis`. Dependency
declarations order selected products; each product's inspection proves required
installed dependencies rather than silently installing unselected products.

The standard invocation starts a detached local process and returns its run ID.
`start SYSTEM... --foreground` waits in the terminal. The process uses no model,
Nucleus job or conversation continuation. Its Python executable and complete
deployment source archive are pinned when the run starts. Updating the checkout
or installing a newer coordinator affects later runs. Python 3.11 or newer,
Git, the normal product build tools, and the current macOS user session remain
host prerequisites. The coordinator must be committed on `main` before its first
run; bootstrap uses the checked-in `deploy.sh`, with no new compiled bootstrap
binary or background daemon.

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
4. Verifies affected Nucleus first. When its inspected contract requires normal
   admission for requester canaries, it releases only Nucleus's hold after that
   proof; all requester holds remain. It then verifies the affected requesters.
5. Releases the remaining run-owned holds only after the required proofs pass.

The first stateful adapters deliberately use a conservative impact closure:
requester selection includes Nucleus, and Nucleus includes all its registered
requester products. Consequently a stateful run can hold and verify unselected
requesters, and all of those installations must already expose compatible
maintenance operations. It never upgrades them implicitly to satisfy that
precondition. Stateless selector-only pilots do not require this bootstrap.

The coordinator owns sequencing and durable evidence. Product adapters own
configuration discovery, admission, quiescence, migration, service control,
domain verification and recovery. Existing deployers retain their ownership,
product and Chancery writer locks. The shared generated profile below stays
limited to products whose deployment only changes program/documentation
selectors. The coordinator does not turn those scripts into stateful deployers.

## Journal and recovery

Private run state is under
`~/Library/Application Support/Cell/deployments/runs/RUN_ID` on macOS. It includes
`run.json`, `run.log`, operation inputs/outcomes, the sealed deployment source,
immutable executable candidates and the disposable preparation worktree. Run
records may contain sensitive operational paths and baseline state; adapters
must never return credentials, domain document bodies or authentication bytes.
Runs and artifacts are retained; this version performs no automatic deletion.

One host-wide deployment lock serializes runs. Each subprocess waits for its
durable PID/start identity before receiving permission to execute and inherits
the lock, so a surviving adapter keeps competing runs out after its parent dies.
The mechanical subprocess helper passes the same descriptor to product
deployers, so a surviving deployer also retains exclusion if its adapter exits.
An unresolved prior maintenance or cutover boundary also blocks new runs. Product
locks and admission gates remain necessary for coexistence with direct commands.

`succeeded` means the required product proofs passed and all remaining run-owned
holds were released. `stopped` preserves the exact incomplete operation and its
holds; an exited runner never becomes success merely because its process ended.
`status` reports an unfinished dead worker as `interrupted` without changing the
journal. `resume` can continue preparation before any maintenance mutation. It
may retain admitted candidates from that same immutable run; no CI pass or
artifact is reused across different runs.

After a mutation or an uncertain response, use `recover`. It refuses to race a
still-running operation. Every affected product must identify a coherent prior
or candidate installation and explicitly prove that releasing maintenance is
safe. Nucleus is recovered first and its own hold released after its proof when
needed for requester recovery canaries; requester holds remain throughout.
After the requester proofs, the coordinator releases remaining holds and records
`recovered`, which is distinct from deployment success. It never repeats an
uncertain apply, clears another owner's hold or silently treats a partial
deployment as an atomic rollback. An unsafe or unavailable recovery stays
stopped for the product's documented recovery procedure.

If Nucleus apply started without a captured successful apply response, recovery
retains its hold and requires the supported Nucleus service recovery procedure.
Selected candidate files, a matching declared version, health and a canary cannot
prove that an old resident daemon was replaced. Nucleus that never began apply
may still prove its unchanged installation; a successful recorded apply allows
the usual service verification. The coordinator records per-product apply-start
evidence durably before launching an installer.

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
  when recovering an interrupted run.

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
`maintenance_products` and `after` as lists of canonical system names. Nucleus
alone may declare `release_after_verify: true` for the explicit service-admission
phase above. Inspection must be read-only with respect to the installation.

Every hold is owned by `run_id`. Hold and release must be idempotent, preserve
pre-existing operator pauses and other runs' holds, and support an interrupted
response. Recovery releases every affected run-owned token idempotently after
proof, including when a hold's effect happened but its reply was lost. Each
mutation must retain enough product-owned evidence for recovery. The coordinator
records `any_apply_started` before permitting any product apply; an unchanged
prior installation may recover pre-cutover holds without inventing new canaries.
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
database work, maintenance, scheduling, authentication, or a domain canary keeps
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
