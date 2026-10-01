# Bazaar

Bazaar stores opaque UTF-8 strings under caller-chosen IDs and immutable
versions. People and programs read the same private SQLite store through the
CLI or an in-process Rust API. Bazaar owns exact stored content and version
identity. Callers own ID meaning, content formats, interpretation, and use.

Read one feature with `chancery show ID`. Read a procedure and its required
feature contracts with `chancery resolve ID`. These commands read installed
documentation. They do not open Bazaar state, establish readiness, or authorize
a content change.

## Features

| ID | Read this to understand |
| --- | --- |
| `bazaar.string.read` | Latest and exact reads, complete version history, CLI results, Rust readers, and query consistency. |
| `bazaar.string.update` | Complete appends, immutable version identity, Rust writers, transaction guarantees, and uncertain completion. |
| `bazaar.installation` | Program publication, private state, initialization, integrity checks, compatibility and program recovery guarantees. |

## Operations

Use `bazaar.install.operate` to install or recover programs, initialize an empty
database, inspect installation, or check state. Its procedure keeps prerequisites,
effects, stop conditions, and verification in place. Its required installation
feature supplies the detailed behavior.

## How the features work together

Initialize compatible private state before opening a reader or writer. A writer
appends one complete string and receives that committed record. A reader can
select its exact ID and version later. A latest read or history query observes
committed state at its own query boundary; separate calls do not share a snapshot.

Every successful append retains another version, even for identical content.
An uncertain receipt requires inspection before another append. Reverting content
means appending an older value. Program recovery preserves the separate database.

Bazaar does not parse prompts, render templates, choose models, migrate callers,
or run agents. It installs no daemon or schedule. There is no supported history
deletion, state migration, or direct SQL integration.

Related feature references provide navigation. Required dependencies express
documentation compatibility and assemble complete contracts; they do not create
runtime calls or transfer authority. The matching Bazaar release publishes these
pages with its provider bundle. Private strings remain outside that publication.
