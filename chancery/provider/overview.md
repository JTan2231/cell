# Chancery

Chancery reads installed, product-owned capability and operation contracts.
It presents the complete catalog and each product's overview. For one selected
entry, it assembles the outward promise, required documentation, exact source
basis, and explicit gaps. The caller selects an entry and separately invokes
the represented interface under that product's authority.

Chancery also owns a private command-usage journal. Participating programs use
its Rust library to register command identities and record observed agent
invocations. These observations do not prove completion or domain success.
There is no daemon, model call, network service, or represented execution path.

## Features

| ID | Read this to understand |
| --- | --- |
| `chancery.directory.discover` | Registry selection, list/product/show/doctor, status meanings, common JSON output, and the public Rust client. |
| `chancery.capability.resolve` | Exact-ID dossiers, normalized facets, dependency closure, basis digests, requirements, and explicit gaps. |
| `chancery.bundle.validate` | Provider and entry format, overview and manual rules, schema compatibility, validation, and release publication guarantees. |
| `chancery.usage.record` | Command attribution, registration semantics, writer and query APIs, journal records, privacy, failures, and limits. |

## Operations

Use `chancery.provider.publish` to author and publish a supported product
contract through its owning release. Use `chancery.installation.operate` to
install or recover the Chancery reader. Use `chancery.usage.operate` to register
program inventories and inspect journal state.

## How the features work together

A product release publishes a self-contained provider bundle. The reader fixes
its installed selector, validates the indexed files, and presents its authored
contracts. Discovery supplies candidates. `show` reads one complete focused
page. `resolve` assembles that page and required feature contracts once each,
while preserving unsupported, unspecified, not-applicable, and undeclared
outcomes. A missing provider affects its own inventory; valid providers remain
available. A resolved promise leaves live readiness unchecked.

The journal is separate from the catalog. CLI dispatch can append a small
observation even when the query itself preserves provider state. Internal
product calls are excluded. Registration is a separate post-install step;
publishing a bundle does not register its program's commands.

Read a feature with `chancery show ID`. Read an operation and its required
feature explanations with `chancery resolve ID`. Related references provide
navigation; only declared version-bounded dependencies affect compatibility.
Owning products retain runtime behavior, access, authorization, recovery, and
domain-success authority.
