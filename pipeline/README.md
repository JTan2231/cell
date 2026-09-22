# Cell pipelines

This directory provides shared product CI and Git release operations. Product
descriptors contain shell assignments only. The pipeline starts with the system
shell and repository tools, without compiling a bootstrap binary. The internal
validator and drift checks require Python 3.10 or newer. The installed CI
manager requires Python 3.11 or newer.

`generate.sh --write` updates checked-in product entry points.
`generate.sh --check` rejects drift. In either mode, repeat `--product PRODUCT`
to select products. The routine `check.sh` body checks descriptor shape,
provider counts, shell syntax, and generated wrapper drift in the light lane.
It does not run regression suites or invoke Cargo. The `test.sh` wrapper uses
the same manager command path as root and product `ci.sh` wrappers. Focused
fixture tests remain development checks, not CI submission.

Each `products/*.sh` descriptor names the product's Cargo packages and manifest,
shell and packaging checks, provider bundles, independently versioned release
units, CI resource class, release branch, deployment profile, and conservative
deployment conflict keys. Product-specific catalog regressions remain small
scripts under `extras/`; arbitrary release or deployment hooks are not part of
the shared format.

`RELEASE_COMPANION_MANIFESTS` optionally lists `release-unit|package-manifest`
rows for provider-owned libraries released at their owner's version. Annals'
`annals-api` and Krisis' `krisis-api` follow this rule; Annals Usage remains an
independent release unit. Release preflight requires matching versions, then
updates, checks, commits, or restores every companion with the primary manifest,
root lockfile, and provider bundles. Companions have no separate release tag.

## Submit CI

Commit the intended changes and submit the commit from the Cell root:

```sh
./ci.sh submit COMMIT
```

`cell-ci submit COMMIT` uses the same installed manager. Root and product
`ci.sh` wrappers provide manager commands only. The manager queues the commit,
integrates it privately, validates it, attempts bounded repairs, deploys the
accepted source, and emails the outcome. See [the CI manager](../ci_manager/README.md)
for setup, effects, status, and recovery. Bare `./ci.sh`, product selection
arguments, and direct validation flags are not supported CI entry points.

## Internal validation

The manager invokes `select_changes.py run` with the fixed job base, the exact
committed candidate, and machine receipts. This is an internal validation
boundary, not a second user CI workflow. The worktree and index must be clean,
and `HEAD` must equal the candidate. A mismatch returns stale state before gate
admission. The validator does not choose either commit from `main` or
`refs/ci/accepted`; the manager supplies them.

The descriptor files define the product inventory. The validator compares the
base and candidate trees. A changed path selects the product whose descriptor
`PRODUCT_DIR` contains it. An edit to `pipeline/products/PRODUCT.sh` selects that
product. Deletions and both paths of a rename count. The same base controls
changed paths, platform classification, and descriptor introductions and
removals. Each repair is committed before validation, and the base stays fixed,
so selection includes the submitted changes and every retained repair.

There are two test groups:

- Product tests cover product commands, APIs, records, rules, and ordinary
  persistence. Every selected product runs these tests, including library and
  binary unit tests, ordinary integration targets, and doctests. Nucleus API
  tests belong here too.
- Platform tests cover installation, upgrades, packaging, maintenance,
  recovery, and shared CI/release/deployment machinery. Cargo integration
  targets named `install` or `maintenance`, installer binary unit tests, and
  the `cell-install` and `cell-maintenance` packages belong here. Shell frontend
  and runner regressions and product catalog regressions also belong here.

`cargo_tests.py` selects Cargo targets from metadata inside the admitted gate.
It does not filter test function names or drop unrelated integration targets.
Formatting, Clippy, provider validation, documentation, release builds, and
binary version checks remain part of each selected product gate. Library unit
tests that mix product and lifecycle behavior still run as a complete target.

`platform_inputs.py` is the explicit platform input map. Product installer
sources, packaging, migrations, schemas, maintenance modules, selected runtime
command files, and operational descriptor edits select that product's platform
tests. Non-version Cargo manifest edits select their owner's platform tests;
root build inputs select the shared build suite. Shared installer changes
select all installer consumers; its common integration fixture selects only
the products that use that fixture. Shared maintenance changes select its
declared consumers. Broker, pipeline,
deployment, build, cleanup, and catalog changes select their own shared suites.
A descriptor absent from the job base selects the new product's platform
tests, the shared pipeline introduction checks, and integrated catalog
validation.

Prose, descriptor comments, and package/provider release-version-only edits do
not select platform tests. Root manifests and lockfiles do not trigger blanket
consumer coverage. Keep installation-affecting shared inputs in the platform
input map.

Mixed runtime/lifecycle files are conservative inputs: any edit to such a file
selects platform tests. Move the lifecycle code to its own module to narrow
that boundary. When adding or moving a platform input or target, update this
map in the same change.

Shared platform inputs can add their affected consumers. There is no general
dependency expansion. The shared suite names are `pipeline`, `broker`,
`deployment`, `build`, `cleanup`, `install`, `maintenance`, `prompts`, and
`catalog`. A validation with no selected products still checks structure and
recognition. Its success does not establish full repository validation.

The validator binds selection and every gate to one source candidate. It
rejects source, Git status, or HEAD changes during planning or execution as
stale. The manager's fixed base controls product, platform, and new-product
selection.

The plan names that baseline and reports platform run/skip reasons. Usher reads
the descriptors' literal assignments without executing them. It checks each
product's identity, Semantics marker, and Chancery introduction. The validator
runs `pipeline/recognition.sh` as a brokered heavy body against its exact source
candidate before the selected product gates. Full Chancery validation remains
in the existing product and integrated catalog gates.

Recognition does not run shared library tests. The `install` and `maintenance`
platform gates format, lint, and test those libraries separately. They are
infrastructure, with no separate product identity or release unit.

The internal dispatcher in `select_changes.py` requests admission from the
host CI broker for each product or shared suite separately. The broker
schedules execution; it does not decide relevance. Product bodies receive
`--tests product|all` as part of their brokered command identity. Product-only
and full gates cannot join each other. An inherited environment flag cannot
bypass admission. The manager validates before acceptance and deployment;
release and deployment preparation do not rerun validation.

The broker captures build and test transcripts. The dispatcher reports
selection before execution and retains the completed product and platform
scope. Failures identify the gate and include bounded diagnostics and a private
log path. The transcript identifies the failed stage. See
[the broker](../ci_broker/README.md) for log bounds and retention.

The manager requests one aggregate JSON receipt on stdout. Progress and
diagnostics remain on stderr. The receipt uses `schema_version: 1` and contains
`state`, `base_commit`, `candidate_commit`, `observed_head`, `source_key`,
`selection`, `gates`, and `failure`. Fields that could not be established are
null. The base and candidate fields name the manager's exact committed range.

The selection records its change mode, coverage mode, product tests, platform
products, shared suites, selection reasons, and ordered `required_gates`.
Each required gate names its gate ID, lane, and command. The `gates` array
contains the broker receipts for gates that ran. A required gate absent from
that array did not complete. The dispatcher stops after the first unsuccessful
gate and checks candidate integrity before it returns the aggregate result.

Aggregate states are `passed`, `failed`, `stale`, `lost`, `cancelled`, and
`error`. A failed gate does not by itself establish a source-code defect.
The `failure` object records its kind, message, gate, and execution ID when
available. Planning, configuration, or invalid broker receipts produce `error`
with exit code 78. Other states retain the broker's exit-code rules. Missing
terminal JSON after process interruption is incomplete validation, never a
pass. The manager owns receipt retention, bounded repair decisions, and the
deployment handoff.

Release and deployment use the shared release builder below. Each product
seals its runtime executables and dedicated `PRODUCT-install`. The coordinator
invokes that sealed installer's Rust adapter. The shared `cell-install` library
owns immutable artifact selection. Product Rust code owns lifecycle and
recovery. Credential and scheduled-job shell frontends remain versioned assets.

`deployment/build.py` requires Python 3.11 or newer. It prepares production
executables without tests, formatting, Clippy, documentation builds, or
generator CI. One release-profile Cargo invocation builds the selected
packages and binaries. The builder then seals product candidates in parallel.
Release builds share a persistent target and file lock per logical Git
repository, separate from CI. Cargo defaults to at most eight jobs.

The cache identifies completed candidates by source content and build inputs.
The builder checks executable hashes and versions before reuse. Git HEAD is
excluded, so a version-update build can be reused after those exact source bytes
are committed. The cache records builds, not CI results. See
[deployment](../deployment/README.md)
for invocation, candidate identity, and cache retention.

`pipeline/release.sh` owns product release publication. It holds one lock in
Git's common directory from preflight through the release build and atomic
push. Immediately before commit, tag, and push, it rechecks `origin/main` and
the release tag. On macOS, `shlock` replaces a lock if its recorded process no
longer exists. Other hosts use a `mkdir` fallback that fails closed. On those
hosts, confirm that no release is active before removing a stale
`.git/cell-release-publication.lock.d`.

## Prompt test state

The shared `prompts` gate checks `cell-prompts`. Changes below `prompting/`
also select all nine prompt consumers in the manager's validation plan. Each
consumer test gate imports `prompting/seed.json` into a private temporary Bazaar database and
sets `CELL_BAZAAR_DATABASE` for its tests. Tests never use the live database.
The importer runs inside the admitted heavy gate. Keep source prompt text in
the explicit seed, not in a test-only runtime fallback.
