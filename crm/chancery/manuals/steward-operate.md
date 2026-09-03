# Install or operate CRM and its steward

This operation covers user-owned installation, explicit database
initialization and diagnosis, and evidence-based recovery of hidden steward
work. It does not authorize release publication, production data mutation,
Nucleus changes, or an otherwise ineligible retry.

## Build before deployment

```sh
./crm/ci.sh
cargo build --release --locked --package crm
crm/packaging/macos/deploy-user.sh \
  --binary /Users/joey/rust/cell/target/release/crm
```

The deployer requires a regular executable at an absolute path. It proves that
the candidate version matches the packaged provider release, validates the
exact bundle/tree and component hashes, stages one immutable content-addressed
release, then publishes command and provider views through one current-release
selector.

Owned paths are:

```text
~/.local/bin/crm
~/Library/Application Support/CRM/install/{current,previous,releases/}
~/Library/Application Support/Chancery/providers/crm
```

It refuses foreign, symbolic, traversal, fabricated, or tampered owned paths.
Identical redeployment is idempotent. An update retains the prior validated
release, and a failed post-switch smoke restores both public views before
commit.

Deployment never creates or opens `crm.db`, launches a worker, restarts
Nucleus, or installs a daemon, LaunchAgent, or schedule.

## Initialize and diagnose

```sh
/Users/joey/.local/bin/crm init
/Users/joey/.local/bin/crm doctor
```

Initialization is a separate intentional effect. It creates schema one when
the selected file is absent and is idempotent against an existing supported CRM
database; it refuses symbolic links and other non-regular targets before
SQLite opens them, refuses foreign or unsupported schemas, and does not
migrate. New Unix database bytes are mode 0600. The packaged default state
directory is mode 0700, while a caller-selected relative database resolves
against the current working directory.

Doctor checks schema identity and the six required tables, SQLite integrity,
foreign keys, secure database/sidecar permissions, strict Nucleus readiness,
and idempotent registration of immutable
`crm/case-steward/1` registration. Storage integrity belongs to CRM; execution
readiness belongs to Nucleus. A Nucleus failure does not make existing cases
unreadable, but hidden steward work cannot progress through a second path.

## Inspect and recover hidden work

Begin with evidence:

```sh
/Users/joey/.local/bin/crm update show UPDATE_ID
/Users/joey/.local/bin/nucleus jobs list \
  --requester crm --requester-id case-steward:UPDATE_ID
```

Use CRM state to decide domain success and retry eligibility. Nucleus job state
is execution evidence. A terminal job without a CRM committed revision is not
success; a CRM commit remains success after a later harness failure. Update
views separately expose successful-result acknowledgment and the retained
terminal runtime state/detail.

For queued or exactly recoverable work:

```sh
/Users/joey/.local/bin/crm update resume UPDATE_ID
```

Resume processes the selected queued, recoverable running, or
applied-but-runtime-unsettled update synchronously and preserves its
requester/job identity. It can recover a durable pending tool call
idempotently, repost the exact committed result, and retain terminal execution
diagnostics. It cannot create a successor update merely because transport or
process state is ambiguous. If wait, resume, or retry fails after resolving an
update, JSON failure output includes a contextual update view with its
attention/advisory; human stderr prints the fixed nonblocking advisory banner
before the error.

For a positively terminal unsuccessful attempt with no committed revision:

```sh
/Users/joey/.local/bin/crm update retry UPDATE_ID
```

Retry reuses the same immutable delivery row/text in a successor update with
new requester/job identities, retains the predecessor through `retry_of`, and
launches the hidden worker. CRM performs no automatic retry.
A Nucleus restart may mark a live harness attempt lost; that is terminal
evidence for CRM to validate, not permission for Nucleus or an operator to
rewrite domain rows.

Hidden drainers serialize through a database-resident lease. A drainer launched
while another owns the lease waits for at most two seconds. The lease owner
performs an atomic queue handoff: a drain owner claims the next eligible update
or releases ownership, and a resume owner releases ownership and starts one
replacement drainer when eligible work is waiting. A bounded contender therefore
cannot open an empty-check/release race.

The hidden Codex invocation uses model `gpt-5.6-terra`, medium reasoning, a
1,200-second timeout, requester `crm`, requester ID
`case-steward:UPDATE_ID`, workspace access `none`, no shell or web, no launch
context, and exactly one managed tool under `crm/case-steward/1`. There is no
direct-Codex fallback.

## Verify a deployment

```sh
/Users/joey/.local/bin/crm --version
/Users/joey/.local/bin/crm doctor
/Users/joey/.local/bin/chancery show crm.case.maintain
/Users/joey/.local/bin/chancery show crm.library.explore
/Users/joey/.local/bin/chancery show crm.steward.operate
/Users/joey/.local/bin/chancery doctor
/Users/joey/.local/bin/nucleus health
```

Run a synthetic isolated canary: create one case, tell it one delivery, retain
the returned update, wait, and prove the exact request/toolset, one guarded
revision commit, replay-safe receipt, visible advisory, read projections, and
Nucleus correlation. Also prove an unsuccessful terminal attempt requires
explicit retry and that completion without CRM commit is not accepted.

Never put real job-search, contact, prompt, or tool-result content in release
bytes, logs, or CI fixtures.

## Rollback

After a committed deployment, use only a valid previous release and its
packaged deployer:

```sh
crm_previous="/Users/joey/Library/Application Support/CRM/install/previous"
"$crm_previous/package/deploy-user.sh" \
  --binary "$crm_previous/bin/crm"
```

Normal ownership, exact-tree, manifest, hash, and version checks apply.
Rollback switches only program/provider selection. It never rewrites CRM or
Nucleus state. Stop when `previous` is absent/invalid or the older binary
cannot read the retained database schema; use that release's database-aware
recovery plan instead of forcing binary rollback.

Version 0.1 has no uninstaller or automatic pruning. Deleting retained cases,
attempts, releases, or a database is a separate destructive action.
