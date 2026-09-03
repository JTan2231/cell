# Publish a product capability to Chancery

This is the canonical cross-system operation for making a current product
capability globally discoverable. It is a semantic operation manual, not an
executable workflow. The interactive agent adapts it to the owning product's
release machinery while preserving the checkpoints and authority boundaries
below.

## 1. Establish ownership and current truth

Start with the user's durable outcome, not a command name or internal tool.
Select `use`, `operate`, or `develop` according to the audience. A capability
that users should request is distinct from an administrative action or a guide
for changing the product.

Read the product's current public contracts. Do not infer supported behavior
from implementation details, old conversations, backlog items, or a prototype.
If the product does not support the outcome today, stop: Chancery is not a
roadmap and must not advertise the aspiration as available.

The product owns claims about its outcome, effects, domain success, recovery,
privacy, and invocations. Chancery owns only the bundle format, validation and
catalog view, deterministic dossier assembly, exact-basis identification,
documentation dependency closure, facet and gap classification, and display.

## 2. Author a self-contained bundle

For provider schema 3, declare what the product is and is not authoritative
for, a meaningful class of public outcomes covered by the inventory, whether
that class is complete or partial, shared access and privacy boundaries,
compatibility and retirement policy, and material system-wide limits. The
inventory scope must not be circular: “everything indexed here” does not say
what absence means.

Add or revise an explicitly indexed entry and its detailed manual under the
product's owned provider source. Normalize consumers, preconditions, inputs,
outputs, data semantics, identity and units, completeness and freshness,
access, lifecycle and consistency, limits, evolution, and substantive
reliances. Each claim must say `declared`, `unsupported`, `unspecified`, or
`not_applicable`; do not infer a positive promise from silence. Keep stable IDs
when semantics and authority remain compatible; increment the contract version
for incompatible semantic changes.

Declare contract-version bounds for other entries whose documented semantics
are required. Those `dependencies` edges mean documentation compatibility,
not runtime calls or data lineage. Publish substantive data, control,
authority, readiness, and external reliances separately. A declared reliance
without a dedicated installed contract remains an intentional resolver gap.

Give every entry a short, discriminative title and a summary that states the
user-visible result clearly enough for an agent to form a semantic shortlist.
Put the detailed positive and negative semantic boundary in `use_when` and
`do_not_use_when`. The manual must make correct use possible without opening
the source tree or checking code.

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

Dependencies within the bundle are checked for contract-version compatibility
and cycles. External dependencies are structurally checked but remain
unchecked in standalone validation. Fix every schema, path, and content issue
before packaging. Unindexed drafts have no effect and should not be used as
evidence that installed discovery works.

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

Confirm that the entry's title and summary are distinguishable in the complete
catalog, that semantic request boundaries and the complete manual render in
`show`, that provider scope, normalized facet coverage, reliance gaps, exact
basis, and dependency closure render in `resolve`, and that no documented
command runs during any Chancery query. The represented product's own readiness
and domain proof still govern an actual invocation.
