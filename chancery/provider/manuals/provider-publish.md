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

For provider schema 3, declare product authority and its limits. Name the class
of public outcomes covered by the inventory and whether coverage is complete
or partial within that class. State shared access, privacy, compatibility,
retirement, and operating limits. Define the inventory class independently of
the index.

Add or revise an explicitly indexed entry and its detailed manual under the
product's owned provider source. Normalize consumers, preconditions, inputs,
outputs, data semantics, identity and units, selected-record coverage and
observation times, access, lifecycle and consistency, limits, evolution, and
substantive reliances. For `completeness_and_freshness`, name the selected
records or operation and explain its counts and timestamps. Use
`not_applicable` when those measurements do not apply. Each claim must say
`declared`, `unsupported`, `unspecified`, or
`not_applicable`; do not infer a positive promise from silence. Keep stable IDs
when semantics and authority remain compatible; increment the contract version
for incompatible semantic changes.

Declare contract-version bounds for other entries whose documented semantics
are required. Those `dependencies` edges mean documentation compatibility,
not runtime calls or data lineage. Publish substantive data, control,
authority, readiness, and external reliances separately. A declared reliance
without a dedicated installed contract remains an intentional resolver gap.

Give each entry a distinct title and a summary of its user-visible result.
An agent must be able to select plausible entries from that text. Use
`use_when` and `do_not_use_when` to define the detailed selection boundaries.

The manual is the complete ordinary `show` view. Include applicability,
interfaces, effects, authority, success, recovery, privacy, exclusions, and
operation checkpoints. Readers must be able to use the interface without the
source tree or structured authoring fields. Keep normalized claims aligned
with the manual for `resolve`. Ordinary `show` does not repeat those claims;
`show --full` includes all authoring fields. Review both views before
publication. Structural validation cannot establish that the prose is complete.

For a cross-capability or UI-dependent procedure, publish an `operation`.
Describe goals, participant capabilities, semantic UI actions, checkpoints,
proof, authorization, adaptation, recovery, and stop conditions. Never encode
volatile selectors, pixel positions, or claims that Chancery will orchestrate
the participants.

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

Stage the exact bundle under the product's content-addressed release, normally
as `share/chancery/PROVIDER_ID`. Include its bytes in the release identity and
integrity manifest. The product installer owns exactly its selector under:

```text
~/Library/Application Support/Chancery/providers/PROVIDER_ID
```

The selector should follow the product's `current` release and roll back with
it. Chancery upgrades preserve the entire providers directory. Product runtime
code must not call Chancery, and product installation must remain useful if the
Chancery binary is absent.

Test fresh install, identical redeploy, upgrade, failed upgrade, rollback,
tamper rejection, and a pre-existing selector owned by something else. Never
silently take over another provider's selector.

## 5. Deploy reader-first and prove installed behavior

When schema compatibility and providers change together, deploy the compatible
Chancery reader first, then provider releases, then any cross-system operation
bundle, and only then make a global bootstrap depend on the new catalog.

After product deployment:

```sh
/Users/joey/.local/bin/chancery doctor
/Users/joey/.local/bin/chancery list
/Users/joey/.local/bin/chancery show ENTRY_ID
/Users/joey/.local/bin/chancery resolve ENTRY_ID
```

Confirm that the title and summary distinguish the entry in the catalog.
`show` must render the request boundaries and complete manual. `resolve` must
render provider scope, normalized facets, reliance gaps, exact basis, and
dependency closure. No Chancery query may execute a documented command.
Actual invocation still uses the product's readiness and domain-success rules.
