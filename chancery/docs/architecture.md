# Architecture

Chancery is a stateless reader over installed provider bundles. It answers
“what capabilities and operations are installed?” and “what does each one
actually promise?” A product release publishes documentation; Chancery lists
it; the interactive agent compares the user's request with the catalog and
separately decides whether and how to invoke a represented interface.

```text
product release -- publishes --> provider bundle -- read by --> Chancery
operation bundle -- references --> capability IDs
Codex -- queries --> Chancery -- then separately invokes --> product or UI
```

Product runtimes never depend on Chancery. Chancery never submits a Nucleus
job, calls a model, opens a network connection, executes a documented
interface, grants authority, or treats runtime readiness as established.

A malformed provider is excluded without hiding valid providers. Any future
index is disposable and derived; provider bundles remain authoritative.

## Authorities

| Concern | Authority |
| --- | --- |
| Supported product outcome, effects, privacy, invocation, and domain success | Owning product and its versioned provider bundle |
| Bundle schema, structural validity, complete catalog enumeration, and presentation | Chancery |
| Whether an installed service, account, UI, credential, or data store is ready now | Represented product or interactive session |
| Whether a requested mutation is authorized | User request plus the represented product's contract |
| Cross-capability choreography | The installed operation manual; each participant keeps its own domain authority |
| Product implementation and release | Owning product |

This prevents Chancery from becoming a second copy of product truth. It also
prevents a syntactically valid document from being mistaken for runtime proof.

## One-way intersystem dependencies

Product source owns a provider bundle. Product CI may invoke `chancery
validate` as a development check. Product packaging copies the unchanged
bundle into the product release and atomically owns one selector under the
Chancery registry. None of those actions adds a Chancery call to the product's
runtime path.

At query time Chancery reads provider files only. It does not call Nucleus,
Todo, Annals, Weaver, Codex, a skill, a browser, or computer use. After reading
a contract, the interactive caller may use an interface named by the contract;
that is a separate action with its own authorization and failure semantics.

Dependencies declared between entries are documentation-contract
dependencies. For example, a requester capability may require the installed
contract for the Nucleus execution capability at version 1. Chancery checks
that the declared contract is present, in range, and itself dependency-
compatible. Unavailability propagates through the installed contract graph and
is displayed on the entry; the entry remains visible in the complete catalog.
Chancery still does not check that the Nucleus daemon is healthy.

## Capability and operation documents

A capability describes one supported outcome owned by one product. Modes keep
audiences distinct:

- `use` is ordinary outcome-oriented work;
- `operate` is installation, readiness, administration, or recovery; and
- `develop` changes an implementation or integration.

An operation describes adaptive choreography across capabilities or volatile
interactive surfaces. It records semantic steps, checkpoints, authorization,
adaptation, recovery, and stop conditions. It is deliberately not an
executable workflow. A computer-use operation can therefore remain useful as
an interface changes, while exact stable CLI invocations remain in the owning
capability contracts.

## Failure isolation

The registry has no database or daemon. Every invocation fixes each provider
selector to one canonical bundle, validates explicitly indexed files, derives
an in-memory view, prints the result, and exits. An invalid provider is reported
and excluded as a unit. Duplicate global entry IDs are not resolved by
filesystem order. Missing or incompatible dependencies affect the referenced
entries without corrupting their provider bundles.

Chancery installation preserves product-owned provider selectors. Each product
release includes its bundle in its own content hash; its installer advances or
rolls back the release and selector coherently. This makes repair local to the
owning product and avoids a central mutable catalog authority.

## Compatible deployment order

When changing the schema or adding providers, use:

1. deploy a reader that accepts the new schema while retaining old support;
2. deploy product bundles and their selectors;
3. deploy cross-product operation bundles after all required capabilities;
4. change the global agent bootstrap last.

Removing or incompatibly changing a contract reverses that logic: update the
agent bootstrap and dependents first, then remove the old provider contract
only when no installed operation requires it.
