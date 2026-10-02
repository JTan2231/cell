# Semantics

Semantics maintains terminology and its history for registered project folders.
It reads accepted documents from one Annals decisions library. A restricted
Nucleus agent proposes changes for each project. Semantics validates those
changes and owns the final append-only repository transaction.

The semantic repository is authority for maintained terminology, concept
identity, and grounding history. Participating products own their runtime
behavior. Annals owns accepted text and feed order. Nucleus owns execution and
mailbox transport. Clockwork owns scheduled activation and failure halts.

Read one feature or procedure with `chancery show ID`. Read a procedure and its
required contracts with `chancery resolve ID`. These commands read the installed
publication. They do not establish readiness, execute a command, or authorize
an effect.

The `semantics` CLI prints plain text by default. Pass `--json` for the existing
command-specific machine responses. Typed clients and the worker explicitly
request JSON.

## Features

| ID | Read this to understand |
| --- | --- |
| `semantics.repository.explore` | Current and historical terminology, stable concepts, typed effects, replay, provenance, and repository output. |
| `semantics.projects` | Participation markers, project identity, activation, moves, pause, retirement, and bootstrap seeding. |
| `semantics.reconciliation` | Accepted-document intake, per-project cursors, agent interpretation, validated commits, no-change results, and safe retry. |
| `semantics.service` | Readiness, serial scheduled work, maintenance holds, release ownership, installation guarantees, rollback, and retained state. |

## How the features work together

Registration captures the current Annals watermark. Earlier documents remain
outside that project's automatic intake. The serial worker saves each later
document as separate project intake before advancing that project's scan cursor.
The agent receives the complete document and repository snapshot. It can propose
semantic effects or return no change. Semantics validates and stores the exact
tool receipt with its domain result before acknowledging it to Nucleus.

Repository reads replay immutable revisions. A pause prevents late proposals
from committing. A runtime failure does not erase an existing semantic commit.
Project pause, deployment maintenance, and a Clockwork incident halt are
separate controls; releasing one does not release the others.

## Operations

Use `semantics.project.operate` to install, verify, register, seed, move, pause,
retire, diagnose, recover, or uninstall Semantics. Its procedures retain the
required prerequisites, effects, stop conditions, and verification. Its required
feature contracts own the detailed behavioral explanations.

Use `semantics.develop.change` to change Semantics. It covers persistent replay,
immutable requester contracts, synthetic validation, packaging, and synchronized
documentation. Read `nucleus manual` for shared maintenance and recovery order.

Semantics publishes this bundle with its content-addressed product release.
Related references provide navigation. Required dependencies declare compatible
documentation and assemble complete reading; they do not transfer authority or
create runtime calls. Catalog presence does not prove live readiness.
