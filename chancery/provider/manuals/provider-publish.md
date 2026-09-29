# Publish a product capability to Chancery

Use this operation to publish a current product capability in the installed
catalog. The interactive agent follows this manual through the owning product's
release process. Preserve the checkpoints and authority boundaries below.
The manual does not execute a workflow.

## 1. Establish ownership and supported behavior

Start with the user's intended durable outcome. Select `use`, `operate`, or
`develop` for ordinary work, administration, or product changes.

Read the product's current public contracts. Do not infer supported behavior
from implementation details, old conversations, backlog items, or a prototype.
If the product does not support the outcome today, stop: Chancery is not a
roadmap and must not advertise the aspiration as available.

The product owns claims about its outcome, effects, domain success, recovery,
privacy, and invocations. Chancery owns only the bundle format, validation and
catalog view, deterministic dossier assembly, exact-basis identification,
documentation dependency closure, facet and gap classification, and display.

## 2. Author a self-contained bundle

Read the required `chancery.bundle.validate` feature for the complete manifest,
entry format, normalization, path rules, schema compatibility, and publication
guarantees. Use `chancery resolve chancery.provider.publish` to include it with
the catalog, resolver, usage, and maintained-terminology contracts.

Declare the product's authority, exclusions, and inventory class independently
of the entry index. State whether coverage is complete or partial within that
class. Select feature boundaries by the coherent outcomes readers need to
understand and rely on. Keep one `capability` entry and full detailed manual for
each feature. Keep its supported interfaces, behavior, data meaning,
authority, lifecycle, failures, recovery, privacy, evolution, and limits there.

Index a schema-4 product overview to explain the whole product and how its
features fit together. Give every entry a discriminative title and summary.
Keep stable IDs and compatible contract versions; increment the contract
version for incompatible meaning. Preserve supported operating routes.

Normalize the same promise that the manual explains. Name the selected records
or operation when describing counts, timestamps, completeness, and freshness.
Retain explicit declared, unsupported, unspecified, and not-applicable claims;
do not infer support from silence. Declare version-bounded required contracts
and substantive reliances separately. An uncontracted reliance is an
intentional resolver gap. Related references provide navigation only.

Publish procedures as operations when they coordinate capabilities or volatile
session surfaces. Keep exact actions, prerequisites, consequential effects,
authority, success evidence, checkpoints, recovery, privacy, exclusions, and
stop conditions in the procedure. Put the full behavioral explanations in its
required feature contracts. The procedure's ordinary `show` page must remain
usable; `resolve` assembles required features once each. Use semantic UI actions
instead of volatile selectors or pixel coordinates. There is no include engine
or workflow executor.

Review `show`, `show --full`, and `resolve`. Keep prose and normalized claims
aligned. Structural validation cannot prove that a manual is complete. Remove
competing explanations from older documents and replace them with entry points
to the product-owned installed content. Keep public details in the published
bundle rather than substituting repository links.

## 3. Validate before changing installed state

Run the candidate reader against the source bundle:

```sh
/absolute/path/to/chancery validate /absolute/path/to/provider-bundle
```

Validation checks internal dependencies for version compatibility and cycles.
For external dependencies, standalone validation checks structure only. Fix
schema, path, and content errors before packaging. Unindexed drafts do not
participate in installed discovery.

## 4. Couple documentation to the product release

Stage the exact bundle as `share/chancery/PROVIDER_ID` under the owning
content-addressed release. Include its bytes in the release identity and
integrity manifest. Follow the required `chancery.bundle.validate` publication
guarantees and the product's own installation contract. The product installer
owns exactly its selector under:

```text
~/Library/Application Support/Chancery/providers/PROVIDER_ID
```

The selector should follow the product's `current` release and roll back with
it. Chancery upgrades preserve the entire providers directory. Product runtime
code must not invoke the Chancery catalog as an execution dependency, and product installation must remain useful if the
Chancery binary is absent.

Test fresh install, identical redeploy, upgrade, failed upgrade, rollback,
tamper rejection, and a pre-existing selector owned by something else. Never
silently take over another provider's selector.

## 5. Deploy reader-first and prove installed behavior

When schema compatibility and providers change together, deploy the compatible
Chancery reader first, then provider releases, then any cross-system operation
bundle, and only then make a global bootstrap depend on the new catalog.

After product deployment, run each updated participating program's exact
`--register-usage` mode through its owning installation procedure. Registration
is separate from bundle publication and binary selection. Preserve existing
history and stop on unsupported journal state; read `chancery.usage.operate`
for recovery.

Read the installed publication:

```sh
/Users/joey/.local/bin/chancery doctor
/Users/joey/.local/bin/chancery list
/Users/joey/.local/bin/chancery product PROVIDER_ID
/Users/joey/.local/bin/chancery list --provider PROVIDER_ID
/Users/joey/.local/bin/chancery show ENTRY_ID
/Users/joey/.local/bin/chancery resolve ENTRY_ID
```

Confirm that the title and summary distinguish the entry in the catalog.
`product` must show the overview exactly when indexed and report its absence
otherwise. Its inventory and the provider-filtered list must identify the
product's entries.
`show` must render the request boundaries and complete manual. `resolve` must
render provider scope, normalized facets, reliance gaps, exact basis, and
dependency closure. No Chancery query may execute a documented command.
Actual invocation still uses the product's readiness and domain-success rules.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.
