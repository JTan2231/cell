# Cell pipelines

This directory owns the repeated mechanics behind product CI and Git release
entry points. Product descriptors remain data-only shell assignments so the
pipeline can start with the system shell and existing repository tools; it
does not need to compile a bootstrap binary. Public CI entry points and drift
checks require Python 3.10 or newer for the broker and generators.

`generate.sh --write` updates the checked-in product entry points; repeat
`--product PRODUCT` to limit either mode to selected products.
`generate.sh --check` rejects drift, and `test.sh` performs the lightweight
descriptor, provider-inventory, shell-syntax, and generation checks.

Each `products/*.sh` descriptor names the product's Cargo packages and manifest,
shell and packaging checks, provider bundles, independently versioned release
units, CI resource class, release branch, deployment profile, and conservative
deployment conflict keys. Product-specific catalog regressions remain small
scripts under `extras/`; arbitrary release or deployment hooks are not part of
the shared format.

The descriptor files themselves are the product inventory. Usher reads their
literal assignments without executing them and checks each product's identity,
Semantics marker, and Chancery introduction evidence. Root CI runs
`pipeline/recognition.sh` as a brokered heavy body against its exact source
candidate before the selected product gates. Full Chancery validation remains
in the existing product and integrated catalog gates.

Product `ci.sh` files are synchronous clients of the host-scoped CI broker.
The broker invokes `pipeline/ci.sh` as the private body. Public product entry
points always request admission; an inherited environment flag cannot bypass
the broker. Root and release orchestration use those public entry points so
each product gate remains an independently scheduled unit.

`pipeline/release.sh` retains product release authority. It holds one lock in
the repository's Git common directory from preflight through CI and atomic
push, and it rechecks `origin/main` and the release tag immediately before
commit, tag, and push. On macOS, `shlock` safely replaces a lock whose recorded
process no longer exists. Other hosts use a fail-closed `mkdir` fallback and
must verify that no release is active before removing a stale
`.git/cell-release-publication.lock.d`.
