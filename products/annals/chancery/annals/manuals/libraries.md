# Annals libraries and instructions

Use this feature to understand library selection, identity, stored instructions,
read access, initialization, supported migration, and statistics.
Use `annals.library.operate` for procedures that change these settings or corpus
history. Each library is a separate local authority. No combined search, graph,
cursor, or transaction across libraries is supplied.

## Library authority and selection

A library contains retained works, source deliveries, instruction revisions,
the corpus, examinations, reconciliations, and append-only corpus history.
Annals owns these records. A consuming application defines the interpretation
of concepts and parent edges through that library's stored instructions.

The corpus consists of concepts, explicit parent edges, and evidence links at
one revision. Retained works, deliveries, examinations, and reconciliations
are library records but are not themselves corpus state. Corpus revision zero
is empty. HEAD is the latest committed corpus revision, not an instruction
revision or a count of deliveries.

The supported selection forms are:

```text
annals library list
annals library create NAME [--kind general|decisions]
annals library NAME show
annals library NAME COMMAND
annals [--config PATH] [--library PATH] [--expected-library-id ID] COMMAND
```

Names contain 1–64 lowercase ASCII letters, digits, underscores, or hyphens,
start with a letter, and cannot be `list`, `create`, or `help`. The private
catalog maps each name to one stable library ID and its managed database,
config, and spool. An unknown name fails without creation. Named scope rejects
`--library`, ignores `ANNALS_LIBRARY`, and verifies the database identity.
An explicit config may tune execution, but cannot redirect the named database
or spool, change its identity, or bypass its admission kind.

Creation reserves one name and identity in `provisioning`, initializes matching
private state with revision zero and default instructions, then marks it
`ready`. Repeating the same name and kind resumes or returns that library.
Conflicting state fails without replacement or rebinding. Creation starts no
model job and creates no schedule. `list` reports registrations and provisioning
state. `show` verifies identity and reports registration, corpus revision, and
selected instruction revision. Neither proves live runtime readiness.

The catalog lives at `ANNALS_STATE_DIR/catalog.db`. The default state directory
is `~/Library/Application Support/Annals` on macOS and `~/.local/share/annals`
elsewhere. New libraries live under `libraries/LIBRARY_ID/` with `annals.db`,
`config.toml`, and `spool/`. The catalog locates a library; it does not duplicate
its source, corpus, or instruction authority. Existing operator-selected paths
remain supported and are not automatically registered. Library rename,
deletion, and automatic adoption of those paths are not supplied.

For operator selection, config resolves from `--config`, then nonempty
`ANNALS_CONFIG`. The library resolves from `--library`, then nonempty
`ANNALS_LIBRARY`, then the selected config's `library`. Missing usable
selection fails with `library_not_configured`. Annals never silently creates
or selects `./annals.db`. Config `expected_library_id` and
`--expected-library-id` can pin an operator path and must agree. Persistent library identity survives supported migration.

The installed macOS frontend selects
`~/Library/Application Support/Annals/config.toml` only when no explicit config
or library selection is present. An explicit library suppresses that default.
The uninstalled executable has no implicit config path. Repository and Linux
uses need selection unless their launcher supplies it. Relative `library` and
`inbox.root` config paths resolve from the config directory; command-line and
environment paths resolve from the process working directory. Bare `annals`
with no subcommand displays help.

## Configuration and common output

An Annals config can select paths and execution settings:

```toml
library = "/absolute/library/annals.db"

[inbox]
root = "/absolute/library/spool"
settle_seconds = 60
minimum_available_bytes = 7_000_000_000

[liaison]
quality = "high"
nucleus_socket = "/absolute/nucleus/nucleus.sock"
```

Unknown config keys are rejected. `expected_library_id` can pin an operator
database. `[decision_feed].expected_library_id` participates in the stricter
decisions-config selection owned by `annals.decision-account.exchange`.
`annals.inbox` owns settling, capacity checks, and inbox limits.
`annals.work.integrate` owns quality presets, an optional `[liaison].model`
override, the Nucleus socket, and liaison timeout. Annals Usage has a separate
config; its selection and fields belong to `annals-usage.consumption.inspect`.
Both programs must select the same reachable Nucleus socket when they inspect
the same execution account.

The common options are `--json`, `--quiet`, and repeatable `-v`. JSON emits one
success object on stdout or one error object on stderr:

```json
{"ok":true,"data":{}}
```

```json
{"ok":false,"error":{"code":"stable_code","message":"description"}}
```

`--quiet` suppresses successful human mutation messages. In human mode, `-v`
prints the resolved library path on stderr. Human rendering escapes control
characters in retained text and labels. Text and JSON select the same content;
output selection changes neither effects nor authority.

| Exit code | Meaning |
| ---: | --- |
| 0 | Success |
| 1 | Unexpected process, I/O, or JSON failure |
| 2 | Invalid command or input |
| 3 | Missing library, work, concept, reconciliation, or revision |
| 4 | Stale state, invariant, or reversion conflict |
| 5 | SQLite, integrity, or history failure |

Public corpus JSON uses concept IDs, labels, exact quotations, edge endpoints,
opaque cursors, and corpus revisions. It exposes no work, reconciliation,
evidence, commit-row, or model-run IDs and no source byte ranges. Source-heading
paths remain public where they locate retained text. Accepted-document exchange
has its own public transport identities and envelopes.

## Immutable admission kind

Each library has one persistent immutable kind: `general` or `decisions`.
Initialization defaults to `general`. Only an explicitly selected separate
producer-accepted decisions setup uses `--kind decisions`. Configuration,
instruction text, named selection, and a direct path override cannot change
this role.

General libraries admit ordinary source documents. Decisions libraries admit
only validated producer deliveries through `annals.decision-account.exchange`.
That feature owns the stricter explicit config, expected identity, spool binding,
and accepted-document feed. A decisions-config inbox run dispatches only
accepted originals or their retry children and leaves `incoming/` unregistered.
Selecting a decisions database directly does not open generic source admission.

## Stored librarian instructions

The instruction interfaces are:

```text
annals library NAME instructions show
annals library NAME instructions history [--limit N] [--before REVISION]
annals library NAME instructions set CONTENT
annals library NAME instructions set --file PATH
annals library NAME instructions set --stdin
```

Operator-selected libraries support the same `instructions` commands without
the named prefix. Setting requires exactly one nonblank UTF-8 document. Annals
preserves its exact bytes and atomically appends and selects one immutable
instruction revision. Identical current bytes return unchanged. Returning to
earlier bytes appends another selection, so A → B → A has three revisions.

The set result contains `changed` and an `instructions` object with `library_id`,
`revision`, `content`, `sha256`, and `recorded_at`. `show` returns the current
exact instructions. History identifies `library_id` and
`current_instruction_revision` and returns newest-first `instructions` and
`has_more`. Its default limit is 20 and maximum is 100. `--before REVISION`
selects strictly older revisions.

Instructions define what the librarian organizes and what concepts and parent
connections mean. The default stored document uses broader/narrower conceptual
scope. This is an interpretation setting, not an unconditional graph invariant.
Annals still enforces immutable sources, valid identities, exact quotations,
an acyclic graph, and evidence on each leaf. Instruction text is trusted
configuration, separate from source evidence.

`recorded_at` describes instruction selection. Selecting instructions starts
no model run, advances no corpus revision, rewrites no committed history, and
schedules no reinterpretation. Future examinations capture their instruction
selection at admission. Historical results can contain earlier selections or
null legacy provenance. Reconciliation inspection reports used and current
instruction revisions separately. `annals.work.integrate` and
`annals.corpus.change` own examination reuse and atomic application checks.

## Initialization, migration, and statistics

The deterministic library interfaces are:

```text
annals init [--kind general|decisions]
annals migrate
annals stats
```

Initialization creates revision zero and returns `library_id` and immutable
kind. It refuses replacement. Library initialization requires initialized
private Bazaar state and a complete `cell.prompts.annals` selection. The default
Bazaar database is `~/.local/share/bazaar/bazaar.sqlite3`; an absolute
`CELL_BAZAAR_DATABASE` override is accepted. Reads fail without creating state
or using embedded fallback text. Bazaar owns the shared selection format,
exact component loading, and rendering. Annals chooses its initial instruction
components and owns their meaning, immutable library capture, initialization,
and domain effects.

Read `bazaar.prompts.prepare` for the shared format and text-preparation rules.
Read `bazaar.prompts.import` for explicit publication and import recovery.
`annals.work.integrate` owns examination context and Annals historical requester
compatibility. Runtime reads and deployment do not import missing prompt contents.

The current library schema is 7. Migration accepts versions 3 through 6 and
applies the missing steps in one transaction. Versions 3 and 4 acquire the
`general` kind; existing later kinds are preserved. The schema-6 step seeds
default instructions at instruction revision 1 and adds frozen examination and
reconciliation instruction provenance. Older records retain null provenance;
migration does not invent their instruction basis or reinterpret the corpus.
The schema-7 step preserves accepted-document sequence and delivery identity
while retaining earlier projections as legacy acceptance records.

Migration changes no retained source or corpus history. Current-format migration
is idempotent. Unsupported older or newer schemas fail without partial mutation.
The installer has no replacement or reset mode. Do not edit or replace an
unsupported active database to bypass the schema boundary.

`stats` is read-only. It reports corpus revision and corpus, graph, work,
reconciliation, history, model-run, and database-size information. These counts
describe the selected library, not model consumption or runtime readiness.
Annals supplies no data backup or restore interface. Existing backup files
remain unchanged.

## Read access and Rust clients

Read commands need only read access to the selected config, catalog, library,
prepared SQLite sidecars, and any spool records and locks used by the command.
Query scratch tables and indexes stay in memory. Reads do not initialize,
migrate, checkpoint, or repair state. Missing required files stop the read.
Initialization, migration, and authorized recovery prepare persistent WAL
files; migration also prepares a configured spool's control lock. Writers keep
readable WAL and shared-memory sidecars after close.

`annals::api` exports provider-owned library, work, corpus, reconciliation,
source-activity, history, and inbox views. Database connections and worker state
remain private. `LibraryReader` uses the same queries and cursor rules with
read-only access. `CliClient` invokes an explicitly selected executable; a typed
`Request` returns its matching `Response`. Requests reuse CLI argument types
and cover reads and mutations, including named libraries and instructions.

`CliClient::for_named_library` selects a registered name. Expected-identity and
state-root options use the CLI checks. Supplying input bytes requires an explicit
`-` input path or `instructions set --stdin`. Client construction has no effects;
each call has the effects of its selected operation. The separate `annals-api`
crate owns accepted-document exchange and usage views.

## Failure, privacy, and compatibility

Unknown names, mismatched identities, conflicting provisioning state, missing
read prerequisites, and unsupported schemas fail without choosing replacement
state. Inspect the selected identity and use the owning operation or installation
recovery route. Do not edit SQLite, the catalog, or spool receipts directly.
Initialization, instruction mutation, migration and corpus
application each require authority appropriate to their effects.

Catalogs, libraries, instructions, spools, and outputs can contain
private paths, source text, quotations, and model context. Keep each under its
local privacy boundary. CLI usage recording requires nonempty `CODEX_THREAD_ID`
and records command identity, time, and thread ID through Chancery. It records
no arguments, output, or outcome; internal product calls are excluded. Recording
errors preserve command results.

Library schema, catalog identity, instruction revision, corpus revision,
provider release, feature contract, CLI output schema, and installed package
generation are distinct compatibility identities. No general migration window,
client support period, ABI guarantee, capacity,
throughput, or wall-clock latency is promised. Library creation and deterministic
administration invoke no model or Nucleus job. Bazaar is required for the
documented initialization text selection.
