# Cell pipelines

This directory provides shared product CI and Git release operations. Product
descriptors contain shell assignments only. The pipeline starts with the system
shell and repository tools, without compiling a bootstrap binary. Public CI
entry points and drift checks require Python 3.10 or newer.

`generate.sh --write` updates checked-in product entry points.
`generate.sh --check` rejects drift. In either mode, repeat `--product PRODUCT`
to select products. `test.sh` submits descriptor, provider-inventory,
shell-syntax, generation, and Python checks to the broker's light lane.
Its private body, `check.sh`, never invokes Cargo.

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

The descriptor files define the product inventory. Usher reads their literal
assignments without executing them. It checks each product's identity,
Semantics marker, and Chancery introduction. Root CI runs
`pipeline/recognition.sh` as a brokered heavy body against its exact source
candidate before the selected product gates. Full Chancery validation remains
in the existing product and integrated catalog gates.
The same root recognition body also formats, lints and tests the shared
`cell-maintenance` library. Usher's product gate checks the shared `cell-install`
library and both `usher` and its product-owned `usher-install` executable.
The shared libraries are infrastructure rather than separate product identities
or release units.

Product `ci.sh` files request admission from the host CI broker and wait for
its result. The broker invokes the private `pipeline/ci.sh` body. An inherited
environment flag cannot bypass admission. Root CI uses the public product
entry points, so the broker schedules each product gate separately. Complete
the relevant CI checks during development. Release and deployment neither
rerun CI nor require a stored CI receipt.

The broker captures build and test transcripts. A direct product gate prints
one success result. Root `./ci.sh` suppresses child success results and prints
the selected scope once. Failures identify the gate and include bounded
diagnostics and a private log path. The transcript identifies the failed stage.
Use `./ci.sh --verbose [PRODUCT...]`, `PRODUCT/ci.sh --verbose`, or
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
