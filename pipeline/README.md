# Cell pipelines

This directory owns the repeated mechanics behind product CI and Git release
entry points. Product descriptors remain data-only shell assignments so the
pipeline can start with the system shell and existing repository tools; it
does not need to compile a bootstrap binary. Public CI entry points and drift
checks require Python 3.10 or newer for the broker and generators.

`generate.sh --write` updates the checked-in product entry points; repeat
`--product PRODUCT` to limit either mode to selected products.
`generate.sh --check` rejects drift. `test.sh` submits the lightweight
descriptor, provider-inventory, shell-syntax, generation, and Python checks to
the broker's light lane; `check.sh` is its private body and never invokes Cargo.

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

The descriptor files themselves are the product inventory. Usher reads their
literal assignments without executing them and checks each product's identity,
Semantics marker, and Chancery introduction evidence. Root CI runs
`pipeline/recognition.sh` as a brokered heavy body against its exact source
candidate before the selected product gates. Full Chancery validation remains
in the existing product and integrated catalog gates.
The same root recognition body also formats, lints and tests the shared
`cell-maintenance` library. Usher's product gate checks the shared `cell-install`
library and both `usher` and its product-owned `usher-install` executable.
The shared libraries are infrastructure rather than separate product identities
or release units.

Product `ci.sh` files are synchronous clients of the host-scoped CI broker.
The broker invokes `pipeline/ci.sh` as the private body. Public product entry
points always request admission; an inherited environment flag cannot bypass
the broker. Root and release orchestration use those public entry points so
each product gate remains an independently scheduled unit.

CI captures build and test transcripts in the broker. A direct product gate
prints one success result; root `./ci.sh` suppresses child success results and
prints the selected scope once. Failures identify the gate and include bounded
diagnostics and a private log path. The pipeline records the failed stage in
that transcript. `./ci.sh --verbose [PRODUCT...]`, `PRODUCT/ci.sh --verbose`,
and `pipeline/test.sh --verbose` request detailed output. `--quiet-result`
suppresses only a successful summary for enclosing orchestration. These are
presentation options and do not change admission identity or gate checks.
See [the broker](../ci_broker/README.md) for log bounds and retention.

For deployment preparation, public product gates accept
`--stage-candidate ABSOLUTE_DIRECTORY`. They run the same complete checks, seal
the exact release executables before their admitted body exits, and return a
broker receipt on success. Staging stays inside the shared Cargo lane; it is
not another build path or a way to skip admission. The deployment coordinator
accepts staged bytes only with the matching passed receipt and fixed committed
source identity. See [deployment](../deployment/README.md).
For Usher, staging seals both `usher` and `usher-install`; the coordinator
invokes the sealed installer's Rust adapter. The recognition executable remains
the read-only Cell membership command.
Presentation options precede `--stage-candidate`; staging always requests the
complete machine receipt even when the enclosing caller suppresses summaries.

`pipeline/release.sh` retains product release authority. It holds one lock in
the repository's Git common directory from preflight through CI and atomic
push, and it rechecks `origin/main` and the release tag immediately before
commit, tag, and push. On macOS, `shlock` safely replaces a lock whose recorded
process no longer exists. Other hosts use a fail-closed `mkdir` fallback and
must verify that no release is active before removing a stale
`.git/cell-release-publication.lock.d`.
