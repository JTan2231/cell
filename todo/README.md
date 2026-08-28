# Todo

Todo is a local Rust CLI for turning a short directional need and the file
where that need arose into one researched, actionable todo. It also supports a
deterministic lifecycle and an optional daily email containing the current open
todos. It is designed for Codex and people with terminal access.

`todo new` invokes a read-only research liaison. The liaison starts with the
source, follows relevant local or external leads, and creates exactly one todo
through a managed tool. The model's prose response is not the result; the
validated tool call is. The source path is retained as provenance, but source
contents are not stored in Todo's database. Nucleus separately retains the raw
agent protocol, which can include content Codex reads during research; its
state therefore belongs inside the same local security and retention boundary.

Todo is deliberately small: SQLite has one table for immutable todo content
and current status, and one append-only table for working notes. There are no
JSON database columns, services, projects, tags, priorities, or status
vocabulary beyond `open` and `done`. The optional macOS schedule is a launchd
timer that invokes the synchronous CLI, not a Todo daemon or queue.

## Requirements

- macOS or Linux and Rust/Cargo 1.97.1 to build;
- a healthy, authenticated per-user Nucleus service for `todo new`;
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
/Users/joey/rust/cell/target/release/todo --database ./todo.db new \
  "Need to report token consumption statistics" \
  --source /absolute/path/to/the-originating-conversation.jsonl
/Users/joey/rust/cell/target/release/todo --database ./todo.db list
/Users/joey/rust/cell/target/release/todo --database ./todo.db show t1
/Users/joey/rust/cell/target/release/todo --database ./todo.db note add t1 \
  "Confirmed that the account allowance has no exposed token denominator."
/Users/joey/rust/cell/target/release/todo --database ./todo.db done t1
/Users/joey/rust/cell/target/release/todo --config ./config.toml email preview
```

The source is usually a Codex conversation transcript, and the command help
says so for agent callers, but it may be any readable UTF-8 file. The direction
is a research lens, not a title or complete specification.

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
`~/Library/Application Support/Todo/install/releases`. Updates switch
`current` and `previous` atomically and restore the prior selector and config
if the installed smoke test fails. The deployer also restores the frontend,
LaunchAgent plist, and loaded-service state. The installation owns
`org.todo.daily-email`, a launchd timer that invokes Todo at 09:00
machine-local time. Email delivery talks directly to Resend; model execution
and authentication remain with the separately installed Nucleus service.
Fresh deployment requires the paired email-address flags shown above; see the
[installation guide](docs/system-installation.md) for the `~/.zshrc` API-key
prerequisite and update behavior.
