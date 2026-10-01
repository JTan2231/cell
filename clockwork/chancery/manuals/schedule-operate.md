# Register and operate Clockwork schedules

Use this procedure for an authorized non-agent product schedule, a deliberate
manual activation, or inspection and recovery of Clockwork evidence. Clockwork
owns activation mechanics and configured halts. The product owns its release,
work, locks, retries, secrets, logs, and domain success. A Clockwork runtime
receipt cannot establish that product work succeeded.

Resolve this entry to read the required `clockwork.definitions`,
`clockwork.bindings`, `clockwork.activations`, `clockwork.incidents`,
`clockwork.notifications`, and `clockwork.installation` contracts. They own
formats and detailed behavior. This procedure owns operation order,
prerequisites, consequential effects, stop conditions, and verification.

Public `clockwork` commands print plain text by default. Add `--json` when
a script or typed integration must parse a result. The flagged response
schemas and the private broker output are unchanged.

## Prepare and register a definition

1. Obtain the product's exact supported runner contract and authorization for
   its scheduled work. Stage and validate its immutable release. Retain its
   prior binding, enabled intent, and rollback basis.
2. Prepare a regular current-user-owned, non-symbolic UTF-8 TOML manifest of
   at most 1 MiB using the definition feature's closed schema. Use canonical
   absolute paths, pinned images and hashes, literal non-secret context,
   private distinct output files, and an interval or local daily trigger.
   Keep group and other write permission absent. Do not supply a mutable
   selector, shell string, inherited environment, or secret.
3. Declare schema-two failure policy. Omission selects the shared service-health
   delay before a halt that requires explicit approval. The first abnormal attempt
   still ends its current run. `continue-next-activation` must be an intentional
   product exception. Keep
   product maintenance engaged throughout selection and rollback.
4. Register the candidate:

   ```sh
   clockwork definition register /absolute/definition.toml
   clockwork definition show DEFINITION_DIGEST
   ```

5. Verify the returned digest and complete retained definition against the
   intended product release. Registration stores metadata without executing
   the runner or changing its selected schedule.

Stop if any artifact, permission, path, hash, policy, or product authority cannot
be proved. Clockwork attests only the registered top-level images, not the
whole release tree or product meaning.

## Select, disable, or recover a binding

1. Inspect the exact key and its incident before changing it:

   ```sh
   clockwork binding show owner/name
   clockwork incident list owner/name --limit 20
   ```

   For a new key, `binding_not_found` is the normal absent prior state. Record
   that absence and continue authorized initial selection. Stop on any other
   read failure; do not infer a missing binding from an unavailable store.
2. Confirm the same-key registered candidate, inactive activation, product
   maintenance gate, prior selection, and macOS current-user GUI domain.
   Switch only the intended key:

   ```sh
   clockwork binding switch owner/name DEFINITION_DIGEST
   ```

3. Verify the returned selected digest and enabled intent. Success confirms
   database selection, generated plist, and loaded launchd state agree. A
   run-at-load candidate or compensated prior selection can request work
   after the transition gate opens. Retain the product gate until coherent
   cutover or rollback is confirmed.
4. Disable when new admission must stop:

   ```sh
   clockwork binding disable owner/name
   ```

   Disable normally waits for a live broker and child to finish naturally.
   If the broker disappeared but its child remains live or cannot be proved
   absent, it rejects and restores the prior coherent binding. Retry only
   after demonstrable child exit. Disable retains selection and history.
5. Restore an exact inactive selection when required:

   ```sh
   clockwork binding disable owner/name --select DEFINITION_DIGEST
   ```

   This form neither loads a schedule nor runs the product.
6. Inspect `clockwork doctor` for a pending transition key when abrupt broker
   loss interrupted cutover. Under product maintenance, rerun the intended
   switch to restore recorded prior state before cutover, or disable to
   consume the journal directly into inactive intent. Verify the receipt.

Stop on unattributable plist or binding bytes, uncertain commit durability,
failed compensation, or unknown process state. Retain the journal and evidence.
Do not edit SQLite or a plist, bypass ownership checks, or load an old and new
schedule together. Product lifecycle tools must serialize their own changes;
stable keys do not authenticate same-user callers.

## Run and inspect

1. Confirm that the product permits a new manual occurrence. Invoke only the
   selected key:

   ```sh
   clockwork run owner/name
   ```

2. Inspect retained evidence:

   ```sh
   clockwork definition list --limit 20
   clockwork binding list --limit 20
   clockwork history owner/name --limit 20 --details
   clockwork doctor
   ```

3. Interpret runtime state separately from product evidence. Nonzero child
   exit can still produce `ok:true` for a durably observed process. A manual
   overlap returns `activation_busy` and records `skipped_overlap` without
   a child. No path retries product work.
4. Read the product's own records when domain success matters. Diagnose a
   failed, timed-out, or lost activation before authorizing another occurrence.

Doctor can prepare private paths, initialize an empty unversioned schema-two
store, and mark a `running` row `lost` only when broker and child are proved
absent. It reports pending transitions without repairing them. For an existing
supported store, `clockwork status-snapshot --json` reads recorded binding,
halt, and runtime metadata without initialization, reconciliation, or launchd
inspection. No check proves the next timer delivery.

## Inspect a halt and approve continuation

New schema-two failures start pending episodes. Later scheduled activations
remain admissible until the shared threshold confirms sustained service failure.
By default, five consecutive failed read-only checks, at least 60 seconds apart,
establish the halt and alert eligibility together. Healthy worker observations,
later successful activations without an abend, and inactive intent clear pending
episodes. Existing halts retain their exact approval rule. Read
`clockwork.incidents` for the full boundary.

1. Inspect the exact retained incident and product failure evidence:

   ```sh
   clockwork binding show owner/name
   clockwork incident list owner/name --limit 20
   clockwork incident show INCIDENT_ID
   ```

2. Preserve committed product results, item recovery state, and user or
   maintenance pauses. Product runners report terminal domain failures with
   `clockwork::api::report_abend(CODE, OCCURRENCE)` and stop successor admission.
   Reports contain only bounded machine code and immutable occurrence ID.
3. Import a legacy failure-owned gate under product maintenance when needed:

   ```sh
   clockwork binding halt owner/name --code legacy_failure --occurrence legacy/ID
   ```

   This explicit halt is immediate and bypasses the new-failure delay. Verify
   the durable incident before removing only the old failure-owned scheduling gate.
   Keep its evidence and all other pauses.
4. Obtain explicit user approval for the exact open incident. Confirm no active
   activation or pending binding transition, then run:

   ```sh
   clockwork binding resume owner/name INCIDENT_ID
   clockwork binding show owner/name
   clockwork incident show INCIDENT_ID
   ```

5. Verify resumption time and cleared `halted_incident`. Approval opens future
   admission only. It does not enable a disabled binding, retry failed work,
   undo product commits, or recover uncertain mail. Installers must not resume.

Switch, disable, restart, rollback, and reinstall preserve incidents. Stop when
product recovery or explicit approval is missing. Do not use notification retry
as scheduling continuation.

## Operate alerts and EMT handoff

1. Confirm compatible installed Iatreion and Email, the stable Cell checkout,
   and standing authority for the retained personal halt alert. Configure or
   advance read-only service checks as needed:

   ```sh
   clockwork notification policy --failure-threshold 5 --interval-seconds 60 --cell-root /absolute/cell
   clockwork notification check
   clockwork notification show INCIDENT_ID
   ```

   Check does not send mail or run product work. Show can save the incident's
   Reply-To metadata. New halts, basic alerts and EMT diagnosis share the
   threshold. Unknown health counts as failed; healthy or explicit inactive
   intent clears pending failure progress. Check can establish a halt at the
   threshold. Existing halts and notification attempts retain their recovery rules.
2. Attempt one due basic notification when needed:

   ```sh
   clockwork notification send
   clockwork incident show INCIDENT_ID
   ```

   Metadata leaves the machine for Email, Resend, and Gmail. Exit zero proves
   provider acceptance, not inbox delivery. Read incident status and attempts;
   the send count is attempted invocations. Broker visits can also advance
   checks and attempt eligible mail. There is no independent timer.
3. Inspect provider acceptance after uncertain transport. After the automatic
   23-hour horizon, or a backwards clock jump before the first invocation,
   Clockwork retains `uncertain`. Obtain explicit approval of duplication risk
   before `clockwork notification retry INCIDENT_ID`. The retry uses a new
   idempotency generation and never clears the halt.
4. Refresh every active pinned broker before enabling EMT. Confirm EMT's own
   supported installation and outgoing-email retention, then configure new
   incident routing:

   ```sh
   clockwork notification emt --receiving-domain RECEIVING_DOMAIN
   clockwork notification show INCIDENT_ID
   ```

   Configuration neither runs nor schedules EMT. Basic fallback waits 120
   seconds after eligibility. EMT's own worker incidents use the basic path.
5. Retain each incident page before advancing its insertion cursor:

   ```sh
   clockwork incident feed --after CURSOR --limit 100
   ```

   Start with `--after 0` for the beginning of retained history. Re-establish
   the cursor after restoring different history. EMT must retain
   its exact outgoing email before claiming initial ownership:

   ```sh
   clockwork notification claim INCIDENT_ID --delivery-id UUID
   ```

   Verify the same delivery UUID. Claim requires eligible, routed, unattempted
   pending mail and refuses a different owner or started basic send. After a
   claim, use EMT evidence for acceptance and its recovery procedure.
6. Disable new preference with `clockwork notification emt --disable` when
   intended. This preserves saved routes and non-expiring claims. Preserve
   database, `failure-checks.json`, both notification sidecars, and EMT
   correspondence together.

Stop on uncertain acceptance, missing retained EMT email, incompatible pinned
brokers, or lost ownership evidence. Do not erase a claim, restore an older
broker that ignores it or run an older broker while `failure-checks.json`
exists. Do not approve unrelated mail or infer mail success from product
runtime state.

## Migrate schema-one state explicitly

1. Hold product schedules and quiesce all Clockwork commands. Settle or recover
   running rows and pending transitions with the old binary. Retain prior
   definitions, selections, enabled intent, product pauses, and release paths.
2. Invoke the tested new exact binary with `clockwork migrate`. Verify the
   returned schema version and retained definition and binding identities.
3. Register and select supported schema-two product definitions under
   maintenance. Preserve disabled selections and import failure-owned halts
   before removing old gates. Verify `failure_policy_active` and refresh all
   enabled broker plists before releasing maintenance.

Program installation never migrates state. Schema-one definitions retain their
old digest and policy. Stop if quiescence, state compatibility, or newer halt
preservation cannot be proved. Use `clockwork.install.operate` for complete
schema compatibility and installation checkpoints.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery records
command identity, time, and thread ID without arguments, output, or outcomes.
Internal product calls are excluded. Recording errors preserve command results.
