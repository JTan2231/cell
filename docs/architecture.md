# Architecture

Todo has one synchronous executable and one local SQLite database. There is no
daemon, scheduler, network service, usage sidecar, or background queue.

Deterministic commands (`list`, `search`, `show`, `note add`, `done`, and
`reopen`) access SQLite directly. `new` additionally starts a Codex app-server
session and exposes a session-scoped `create_todo` tool. A successful,
validated tool call is the creation result; the liaison's final prose is only
diagnostic.

## Creation flow

1. Resolve the source to an absolute path and validate that it names a readable
   UTF-8 file. The bytes are not retained or copied into the initial prompt;
   the liaison reads the path during its research.
2. Start Codex from the caller's working directory with broad read access.
3. Supply the source path, caller working directory, direction, and the stable
   research prompt.
4. Allow the liaison to inspect the source and pursue relevant local or web
   research.
5. Validate one `create_todo(title, note)` call and insert its content with
   host-supplied provenance, status, and timestamps in one transaction.

The liaison is intentionally not confined to the source. References in it are
research leads, and the liaison may inspect code, documentation, tests, Git
history, other conversations, and external materials when they materially
improve scope or completion criteria.

Read access is not guarded by a Todo-specific allowlist. The process may use
the normal research facilities available to Codex. Its write boundary is
strict: project files and external systems are read-only, and the only
state-changing application tool exposed by Todo is `create_todo`. The liaison
does not receive shell-based write authority or general-purpose external
mutation tools.

Both Codex child commands remove any inherited `CODEX_EXEC_SERVER_URL` and run
with an isolated home containing only file-backed authentication. Todo does not
select a remote Code Mode host. Thread and turn requests omit environment
overrides so the isolated process uses its default local environment; an empty
environment list would disable the local read surface as well. This preserves
local research without allowing a caller's remote executor or saved
environment configuration to replace the read-only sandbox.

The host, not the liaison, supplies `pointer`, `source_path`, `status`, and
timestamps. A second creation call is rejected. If Codex stops without a
successful call, no todo is created. If the insertion commits and Codex then
fails while returning its diagnostic prose, the durable creation wins and is
reported.

Research is proportionate rather than mechanically bounded. The prompt tells
the liaison to stop when more investigation is unlikely to materially improve
the intended outcome, current-state understanding, affected parties and
systems, constraints, dependencies, or verification criteria.

SQLite uses foreign keys, a busy timeout, WAL journaling, read-only connections
for reads, and immediate transactions for writes. These are operational
details, not additional domain concepts.
