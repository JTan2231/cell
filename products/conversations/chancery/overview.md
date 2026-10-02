# Conversations

Conversations reads local Codex task history for the current user. It is a
short-lived CLI and Rust adapter. It owns filters, normalized user and assistant
messages, stable references, fork deduplication, completed-turn activity, and
explicit failures when a requested read is incomplete. Codex App Server owns
discovery, stored metadata, source records, pagination, and storage compatibility.

Conversations has no daemon, database, persistent index, authentication flow,
model workflow, scheduler, or network service. It never opens Codex JSONL or
SQLite storage and does not mutate tasks or determine their meaning.

Read one feature or procedure with `chancery show ID`. Read its required
contracts with `chancery resolve ID`. These commands read installed documentation;
they do not read history, establish readiness, or authorize an operation.

## Features

| ID | Read this to understand |
| --- | --- |
| `conversations.history.explore` | Task metadata, normalized history, exact references, filters, search and export, completed-turn activity, and completeness. |
| `conversations.runtime` | Executable selection, diagnostic privacy, private process cleanup, readiness, release identity, and installation recovery guarantees. |

## Operations

Use `conversations.installation.operate` for an authorized installation or
recovery from validated artifacts. Use `conversations.develop.change` for
changes to Conversations and its public contracts. Ordinary CI delivery uses
the installed Cell manager; manual deployment and release publication require
their applicable authority.

## How the features work together

Each operation selects one Codex executable and starts one local App Server.
Ordinary reads use its state-database metadata without repair. History reads
normalize selected user and assistant items; activity provides content-free
completed-turn metadata. Only explicit `refresh` permits App Server metadata
repair. Conversations cleans up its own process group when the operation ends.

The Rust library supplies typed values to local products. Installing the CLI
updates its published contracts and installed selectors. Embedded consumers
must be rebuilt and deployed to receive library changes.

Results can contain private titles, working directories, references, and
transcript text. Callers own redirected output and inherited diagnostics.
Machine-wide liveness and future App Server compatibility are not promised.
The shared `nucleus manual` owns ecosystem coordination and recovery order.
Required documentation dependencies do not transfer runtime or domain authority.
