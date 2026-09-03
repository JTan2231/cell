# Data model

Clockwork uses one private SQLite database at
`~/Library/Application Support/Clockwork/clockwork.db` by default. Schema
version 1 has three durable aggregates: definitions, bindings, and activations.

## Definitions

A definition is a complete immutable snapshot of one stable key's schedule,
launch image, literal arguments, working directory, non-secret environment,
output paths, authority, timeout, and overlap policy. Its identity is the
SHA-256 digest of the canonical JSON encoding of the fully concrete normalized
definition content. Registration either returns the existing identical
identity or inserts a new row; it never mutates an existing
definition.

Definitions also retain the canonical release root, whose final component is
the product's caller-supplied exact 64-lowercase-hex content release identity.
Clockwork pins that identity but does not recompute or attest the whole release
tree. Direct definitions retain one canonical program path and SHA-256.
Interpreted definitions retain separate canonical interpreter and script paths
and SHA-256 values. Direct programs carry a recognized Mach-O/fat magic;
schema-one interpreted definitions use only the exact root-owned `/bin/sh`
profile. The current-user-owned executable script stays beneath the product
release. These fields describe
only the top-level launch images.
They do not inventory transitive libraries, later-opened configuration, or
subprocesses.

## Bindings

A binding gives the stable `owner/name` key one nullable selected definition.
The selected definition must carry the same key. Disabling an existing binding
retains its selected digest; disabling an absent key creates a tombstone with a
null selection. Both retain history.
The recovery form `binding disable --select` may replace that inactive selection
with another registered same-key definition without enabling it.

An enabled binding also retains the SHA-256 of the exact generated plist bytes.
Clockwork checks that private identity before replacing, unloading, or removing
the file. During an incomplete disable it retains the digest until external
cleanup completes. The digest is internal ownership evidence rather than a
product definition identity.

Database selection, generated plist bytes, and launchd loaded state form one
operational consistency unit during `switch` and `disable`, even though
launchd is outside SQLite. Clockwork serializes the transition with a per-key
management lock plus the activation transition gate, keeps the exact prior
external and database state, and compensates on
failure. It never treats a database-only write as a successful cutover.

Before the first mutation of a binding or launchd projection that already
exists, Clockwork fsyncs one private per-key transition journal beside its lock
files. Disabling a wholly absent coherent key is only one atomic SQLite
tombstone write and needs no cross-view journal. The journal otherwise contains the
operation, target definition digest, prior binding row, exact prior plist
bytes, prior loaded-state observation, and the exact candidate definition
digest and plist bytes for a switch.
Success removes and fsyncs that journal only after all three views agree. A
later switch for the same key first restores any retained prior state under
both locks. A later disable first replaces that journal with a durable disable
intent while preserving candidate attribution, then consumes it directly into
the requested disabled selection without loading either generation. Rerunning one
of those operations is the supported abrupt-broker recovery path. `doctor`
reports pending keys but does not choose between those effects.
Recovery refuses to overwrite or remove a current plist or binding that
matches neither the recorded prior nor candidate projection. If unlinking the
journal succeeds but syncing its directory does not, the coherent projection
is retained and reported as durability-uncertain instead of being rolled back
without a journal.

## Activations

One activation pins one definition and represents at most one direct child
execution. It is not a retry container. Stored fields include:

- stable activation, binding, and definition identities;
- invocation source (`manual` or `launchd`);
- admission, start, and finish timestamps where applicable;
- broker and direct-child PID while known; the direct child is its process-group
  leader and begins as a blocked Clockwork exec gate before the registered
  image replaces it at the same PID;
- state: `running`, `start_failed`, `exited`, `signaled`, `timed_out`,
  `skipped_overlap`, or `lost`;
- exit code, terminating signal, and optional detail where applicable.

Terminal states are append-like runtime history and are not rewritten into a
product-domain result. A zero exit remains `exited`; it does not become
`succeeded`. Clockwork stores neither stdout/stderr bodies nor a secret value.
The manifest stores distinct absolute product-owned output paths. Clockwork
checks their canonical symlink-free, owner-writable/searchable private parent,
opens an existing private owner-writable regular non-hard-linked file for
append or creates it mode 0600, verifies the opened stdout/stderr inode
identities differ, and leaves content and retention to the product.

An interrupted Clockwork process can leave `running` history. A later broker,
binding transition, or doctor may mark it `lost` only after proving the
recorded broker and any recorded child are both absent; age alone is
insufficient. Doctor therefore has this bounded recovery write in addition to
its diagnostic reads.

The broker records the gate PID before allowing product execution. A private
status marker starts pending, is claimed and unlinked by the gate, and is
cleared immediately before `exec`; explicit validation, output-open, and loader
failures are returned through that marker rather than product output. If the
broker disappears earlier, the gate observes EOF and exits without executing
the product. If it disappears after release, the durable child PID prevents a
second admission until that process is demonstrably absent.

## Consistency and privacy

Writes use foreign keys, transactions, a bounded busy timeout, and private
database and parent-directory modes. Per-key filesystem locks coordinate
separate short-lived broker processes. Product durable work must not be stored
inside the Clockwork transaction or inferred from it.

A fixed `clockwork_meta` marker distinguishes schema one from an unrelated or
partial SQLite database. Clockwork initializes only an empty unversioned file
and refuses unknown versions or missing schema-one objects; opening the store
does not relabel or fill in an incompatible database.

Schema version 1 has no automatic migration. A future schema change must ship
an explicit migration and a quiescent database-plus-sidecar backup and rollback
procedure. Deployment of Clockwork program bytes does not initialize, migrate,
delete, or prune this state. The program deployer separately validates the
exact staged Chancery provider through an explicitly supplied candidate reader
before changing its command or provider selectors.

Definition records necessarily reveal local paths, artifact digests, literal
arguments, schedule, and scrubbed environment. Activation records reveal when
and how direct processes ran. State therefore remains private to the current
user. Product secrets and output bodies are prohibited because local file mode
is not a secret-management or content-retention policy.
