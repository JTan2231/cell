# Cell pipelines

This directory provides shared product CI and Git release operations. Product
descriptors contain shell assignments only. The pipeline starts with the system
shell and repository tools, without compiling a bootstrap binary. Public CI
entry points and drift checks require Python 3.10 or newer.

`generate.sh --write` updates checked-in product entry points.
`generate.sh --check` rejects drift. In either mode, repeat `--product PRODUCT`
to select products. The routine `check.sh` body checks descriptor shape,
provider counts, shell syntax, and generated wrapper drift in the light lane.
It does not run regression suites or invoke Cargo. `test.sh` is now an explicit
request for shared platform suites; it is not the routine preflight.

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

The descriptor files define the product inventory. Root `./ci.sh` selects
products with staged, unstaged, or nonignored untracked changes relative to
`HEAD`. A changed path selects the product whose descriptor `PRODUCT_DIR`
contains it. An edit to `pipeline/products/PRODUCT.sh` selects that product.
Deletions and both paths of a rename count. Committed branch changes do not
count as outstanding changes.

Use `./ci.sh` for routine validation. Agents use `--all` only when the user
explicitly requests full CI.

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
the products that use that fixture. Shared
maintenance changes select its declared consumers. Broker, pipeline,
deployment, build, cleanup, and catalog changes select their own shared suites.
A descriptor absent from `HEAD` selects the new product's platform tests, the
shared pipeline introduction checks, and integrated catalog validation.

Prose, descriptor comments, and package/provider release-version-only edits do
not select platform tests. Root manifests and lockfiles do not trigger blanket
consumer coverage. If a shared dependency change affects the installation
boundary, request `--platform` for the affected products.
Mixed runtime/lifecycle files are conservative inputs: any edit to such a file
selects platform tests. Move the lifecycle code to its own module to narrow
that boundary. When adding or moving a platform input or target, update this
map in the same change.

Explicit product arguments run those products even when their source is clean.
They limit product coverage; CI reports affected platform products outside the
requested scope. Without explicit arguments, shared platform inputs can add
their affected consumers. There is no general dependency expansion.

| Command | Coverage |
| --- | --- |
| `./ci.sh` | Changed products, plus platform coverage selected from the same changes |
| `./ci.sh todo` or `todo/ci.sh` | Todo product tests, plus selected platform coverage |
| `./ci.sh --platform todo` or `todo/ci.sh --platform` | Todo product and platform tests, plus shared installation primitives |
| `./ci.sh --all` | Every product and shared platform suite, including integrated catalog validation |
| `./ci.sh --platform` | Explicit full coverage, equivalent to `--all` |
| `pipeline/test.sh broker` | Explicit broker regression suite |
| `pipeline/test.sh` | All shared platform suites |

The shared suite names are `pipeline`, `broker`, `deployment`, `build`,
`cleanup`, `install`, `maintenance`, and `catalog`. `--all` cannot be combined
with product arguments. `--verbose` works with each mode. A root run with no
selected products still checks structure and recognition. Its success does
not establish full repository validation.

Root CI binds the selection and every gate to one source candidate and rejects
source, Git status, or HEAD changes during planning or execution as stale. The
same comparison to HEAD controls product, platform, and new-product selection.
The plan names that baseline and reports platform run/skip reasons. Usher reads
the descriptors' literal assignments without executing them. It checks each
product's identity, Semantics marker, and Chancery introduction. Root CI runs
`pipeline/recognition.sh` as a brokered heavy body against its exact source
candidate before the selected product gates. Full Chancery validation remains
in the existing product and integrated catalog gates.
Recognition does not run shared library tests. The `install` and `maintenance`
platform gates format, lint, and test those libraries separately. They are
infrastructure, with no separate product identity or release unit.

Root and product `ci.sh` entry points use one dispatcher in `select_changes.py`.
It selects coverage and requests admission from the host CI broker for each
product or shared suite separately. The broker schedules execution; it does not
decide relevance. Product bodies receive `--tests product|all` as part of their
brokered command identity. Product-only and full gates cannot join each other.
An inherited environment flag cannot bypass admission. Complete
the relevant CI checks during development. Release and deployment neither
rerun CI nor require a stored CI receipt.

The broker captures build and test transcripts. A direct product gate prints
one success result. Root `./ci.sh` suppresses child success results, reports
selection before execution, and prints the completed product and platform
scope on success. Direct product commands use the same policy within their
explicit product scope, without repeating root structure and recognition.
Failures identify the gate and include bounded
diagnostics and a private log path. The transcript identifies the failed stage.
Use `./ci.sh --verbose [PRODUCT...]`, `./ci.sh --all --verbose`,
`PRODUCT/ci.sh --verbose`, or
`pipeline/test.sh --verbose` for detailed output. `--quiet-result` suppresses
only the success summary for an enclosing caller. These options change
presentation only.
See [the broker](../ci_broker/README.md) for log bounds and retention.

Use `--stage-candidate ABSOLUTE_DIRECTORY` with a public product gate to prepare
a candidate through full CI. The gate runs all checks, seals the release
executables before its admitted body exits, and returns a broker receipt on
success. Staging uses the shared Cargo lane and requires broker admission.
Release and deployment use the shared release builder below. Each product
seals its runtime executables and dedicated `PRODUCT-install`. The coordinator
invokes that sealed installer's Rust adapter. The shared `cell-install` library
owns immutable artifact selection. Product Rust code owns lifecycle and
recovery. Credential and scheduled-job shell frontends remain versioned assets.
Presentation options precede `--stage-candidate`; staging always requests the
complete machine receipt even when the enclosing caller suppresses summaries.

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
