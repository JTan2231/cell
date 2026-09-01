# Todo

Todo is a local Rust CLI for retaining an actionable concern from the place
where it arose, deciding which durable todo it belongs to, assessing the
current situation, and reconciling a design. It is designed for Codex and
people with terminal access.

Todo keeps its durable layers separate:

- a `cN` concern preserves the caller's direction and source provenance;
- a pending `rN` routing proposal says whether that concern should attach to,
  create, revise, unify, dismiss, or defer a todo identity;
- a `tN` todo is the stable umbrella for an enduring actionable concern, whose
  current title and direction come from its latest direction revision;
- an `aN` situation assessment describes dated observed state and authority,
  while a `dN` design describes a proposed or explicitly accepted desired
  state.

Those layers are deliberately not an implementation workflow. Todo does not
model plans, work items, implementation execution, or a general project graph.
Nucleus jobs used to research routing, assessments, and designs are runtime
provenance, not execution of the todo. Existing `done` and `reopen` commands
continue to maintain an umbrella's small `open`/`done` lifecycle.

`todo new` is a convenience for `concern add` followed by `concern assess`.
The concern is committed before research begins. The routing liaison reads the
source and a bounded snapshot of candidate todos, then records one pending
proposal. It cannot apply the proposal. `routing accept --source PATH` is a
separate, provenance-bearing authorization command.

Situation and design research have the same boundary. `todo assess tN` records
an immutable dated assessment whose jurisdictions can name one owner plus
participants and consumers. Bounded source reads use stable source IDs, and
the assessment persists the exact `source_ref` mapping behind every source
citation. A newer assessment makes every older `aN` non-current.

`todo design propose tN` resolves and rechecks the latest current ready
assessment, then records a draft bound to that exact `aN`. The draft explicitly
keeps, moves, adds, or retires each jurisdiction and may cite only its closed
direction, assessment, predecessor, and correction basis catalog. Ready means
all nine desired-state clause kinds and the full direction and predecessor are
covered, not merely that no choices remain. Correction feedback is immutable;
correction leaves the named design unchanged and produces a successor. A
liaison that stops with an open draft leaves an inspectable `abandoned` draft
that can be corrected. Only `design accept --source PATH` can authorize a ready
design, and only while its umbrella remains open and canonical and its
assessment current. Acceptance does not plan or execute implementation.

Todo stores source paths, decision-source paths, and evidence references, not
the contents of those files. Nucleus separately retains the model's raw JSONL
output, which can include content Codex reads during research; its state
therefore belongs inside the same local security and retention boundary.

## Requirements

- macOS or Linux and Rust/Cargo 1.97.1 to build;
- a healthy, authenticated per-user Nucleus service for routing, assessment,
  design, and `todo new` research;
- no separate Todo daemon or database server;
- for email sending only, a Resend API key and a Resend-verified sender domain.

Todo is a member of the Cargo workspace rooted at `/Users/joey/rust/cell`.
The root workspace supplies its Nucleus client, contract, and third-party
dependencies while Todo continues to own its domain behavior and state.

## Build and use

```sh
cd /Users/joey/rust/cell/todo
./ci.sh
cargo build --manifest-path ../Cargo.toml --package todo --release

/Users/joey/rust/cell/target/release/todo --database ./todo.db init
/Users/joey/rust/cell/target/release/todo --database ./todo.db concern add \
  "Need to report token consumption statistics" \
  --source /absolute/path/to/the-originating-conversation.jsonl
/Users/joey/rust/cell/target/release/todo --database ./todo.db concern assess c1
/Users/joey/rust/cell/target/release/todo --database ./todo.db routing show r1
/Users/joey/rust/cell/target/release/todo --database ./todo.db routing accept r1 \
  --source /absolute/path/to/the-authorizing-conversation.jsonl
/Users/joey/rust/cell/target/release/todo --database ./todo.db show t1
/Users/joey/rust/cell/target/release/todo --database ./todo.db assess t1
/Users/joey/rust/cell/target/release/todo --database ./todo.db design propose t1
/Users/joey/rust/cell/target/release/todo --database ./todo.db design accept d1 \
  --source /absolute/path/to/the-authorizing-conversation.jsonl
```

The source is usually a Codex conversation transcript, but it may be any
readable UTF-8 file. A source path says where a statement or decision came
from; Todo does not treat the source contents as instructions and does not
reopen them on ordinary reads.

Select the SQLite database explicitly with `--database` or `TODO_DATABASE`, or
select a strict TOML config with `--config` or `TODO_CONFIG`. The development
binary does not silently create `./todo.db`. Every command supports stable
human output and `--json` output.

See [the documentation index](docs/README.md) for the complete CLI, liaison,
data, and installation contracts.

## Release

Todo releases use annotated tags named `todo-vMAJOR.MINOR.PATCH`. From a clean
`main` branch that exactly matches `origin/main`, run one of:

```sh
./release.sh --patch
./release.sh --minor
./release.sh --major
```

The script bumps `crates/todo/Cargo.toml`, refreshes the root workspace
`Cargo.lock`, runs Todo's complete CI suite, verifies the release binary
version, commits and tags the release, and atomically pushes `main` with its
tag.

## User-owned macOS deployment

After a release build, deploy without administrator privileges:

```sh
./packaging/macos/deploy-user.sh \
  --binary "/Users/joey/rust/cell/target/release/todo" \
  --email-from 'todo@joeytan.dev' \
  --email-to 'j.tan2231@gmail.com'
```

The sender and recipient above are configuration for this deployment, not
universal Todo defaults.

This installs `~/.local/bin/todo`, initializes
`~/Library/Application Support/Todo/todo.db`, and stores complete,
content-addressed releases under
`~/Library/Application Support/Todo/install/releases`. During an update the
deployer quiesces the email LaunchAgent, retains a transaction-local database
backup, explicitly migrates the database, and runs the installed smoke test.
If a later step fails, it restores both database and installed release state.

The installation owns `org.todo.daily-email`, a launchd timer that invokes
Todo at 09:00 machine-local time. Email delivery talks directly to Resend;
model execution and authentication remain with the separately installed
Nucleus service. Fresh deployment requires the paired email-address flags
shown above; see the [installation guide](docs/system-installation.md) for the
`~/.zshrc` API-key prerequisite and update behavior.

The message is a daily attention digest. It groups pending concerns and open
canonical todos under **Needs your decision**, **Needs follow-up**, and **Other
open todos**. Entries lead with a title or plain-language label and status;
typed references such as `Todo tN` and their safe inspection commands are
secondary. The digest does not include concern bodies, directions, notes,
source paths, assessment or design summaries, unresolved choices, or evidence.
