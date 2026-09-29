# Change Clockwork

Use this procedure for an authorized change to Clockwork source, tests,
packaging, or documentation. Read product instructions and the required feature
contracts: `clockwork.definitions`, `clockwork.bindings`,
`clockwork.activations`, `clockwork.incidents`, `clockwork.notifications`, and
`clockwork.installation`. They own behavior. Read `chancery.provider.publish`
for publication rules and `semantics.repository.explore` for terminology.

## Prepare the change

1. Query Cell for shared maintained terminology until the `clockwork` Semantics
   project is explicitly registered and seeded. After that transition, use its
   registered repository. Source, tests, and product contracts remain behavior
   authority. Do not register, seed, or edit semantic state as a side effect.
2. Identify the smallest affected boundary, supported interfaces, persistent
   meaning, compatibility axes, authority, privacy, and recovery obligations.
   Read `nucleus manual` before public, persistent, deployment, or integration
   changes. Use its applicable change playbook.
3. Preserve the short-lived current-user broker scope. Runtime callers supply
   only stable keys. Products keep durable work, retries, locks, secrets, output
   retention, and domain success. launchd keeps timer-delivery authority.
4. Implement the owning change and update its feature contract, relevant
   procedure, overview, and release packaging together. Keep README and legacy
   references as entry points. Required contract dependencies must remain
   acyclic. Related reading does not imply runtime calls or compatibility.

Stop for a new boundary review before a daemon, network surface, agent runner,
arbitrary command interface, workflow, work queue, retry/backoff engine, secret
store, output capture, distributed coordination, system service, or domain
success policy is introduced.

## Validate the affected promises

1. Use synthetic release and state roots, launchd doubles, and child processes.
   Fixtures contain no credential, private path, production definition, output,
   or activation history. Diagnostics identify fields and stable IDs without
   echoing full environment or product output.
2. Select checks for the changed boundary. Preserve strict manifests, immutable
   identities, direct-image containment and hashes, stable-key-only runtime
   invocation, overlap, process-group supervision, termination, timeout, and
   proof before lost recovery. Do not attest transitive execution or treat exit
   zero as domain success.
3. Exercise binding first enable, update, idempotence, active refusal,
   bootout/bootstrap failure, compensation, journal recovery, and attributable
   fail-disabled behavior when affected. Never permit intentional dual schedules.
4. Use a successor schema, explicit quiescent database-plus-sidecar backup,
   migration, old-state fixture, and database-aware rollback for persistent
   meaning changes. Retain old definition identity and decoding. Deployment
   must not migrate storage implicitly or clear an incident.
5. Check policy changes against nonzero exit, startup failure, signal, timeout,
   lost proof, reported failure with exit zero, overlap, re-observation after
   approval, and halt preservation. Use only local Email doubles. Preserve
   fixed payload, idempotency horizon, eligible checks, and non-expiring claims.
6. Check packaging version matching, complete content identity, candidate-reader
   validation before selector mutation, installed discovery, idempotent
   deployment, foreign/tampered refusal, rollback, and selector-only uninstall
   when affected. Validate the source provider and review product, show, and
   resolve with a temporary registry. Preserve explicit unknown and
   uncontracted reliance gaps; structural validation cannot prove prose complete.

Stop when migration, compatibility, external agreement, ownership, privacy, or
coherent recovery evidence is missing. Keep every affected contract consistent.

## Deliver when authorized

1. Commit the intended change and submit `./ci.sh submit COMMIT` from the Cell
   root. Verify the retained manager outcome. Focused checks do not replace
   manager validation and delivery.
2. Treat `release.sh` as separate publication authority: it commits, tags, and
   pushes. Manual deployment outside a submitted job, real binding operation,
   destructive state work, and semantic registration or seeding are separate
   operations with their own authority.
3. Report the verified endpoint and material remaining limits. If the user
   requested an uncommitted review, leave the reviewed changes uncommitted and
   report that manager validation and deployment remain pending.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery retains
command identity, time, and thread ID without arguments, output, or outcomes.
Internal product calls are excluded. Recording errors preserve command results.
