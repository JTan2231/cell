# Operate an Annals library

Use this operation to create or configure a library, inspect or back up its
state, or make an explicitly authorized corpus-history change. Read the required
`annals.libraries` and `annals.corpus.change` feature contracts for selection,
stored data, reconciliation format, limits, consistency, and recovery meaning.
This procedure invokes no AI reader and does not operate the inbox.

Select a registered library with `annals library NAME COMMAND` or an explicit
operator config or path. Verify its identity and immutable kind before mutation.
Read operations need readable prepared state and never create or repair missing
sidecars or locks. Initialization and backup require absent destinations.
Keep libraries, source text, instruction history, model context, and backups private.

## Create or select a library

1. List registrations and inspect the intended target:

   ```sh
   annals library list
   annals library NAME show
   ```

2. Create a new name only when that library is intended:

   ```sh
   annals library create NAME [--kind general|decisions]
   ```

3. Repeat the same name and kind to resume interrupted provisioning. Stop on
   conflicting state or identity; do not replace it. Creation starts no model
   or schedule. Verify `ready`, library ID, corpus revision, and instruction
   selection with `show`. These values do not prove model or scheduler readiness.

## Select librarian instructions

1. Read the current selection and history:

   ```sh
   annals library NAME instructions show
   annals library NAME instructions history --limit 20
   ```

2. Set exactly one nonblank UTF-8 document when an instruction change is authorized:

   ```sh
   annals library NAME instructions set 'Complete librarian instructions'
   annals library NAME instructions set --file /absolute/instructions.md
   annals library NAME instructions set --stdin
   ```

3. Verify `changed`, exact selected text, SHA-256, and instruction revision.
   Identical current bytes are unchanged. A new selection starts no examination,
   changes no corpus revision, and rewrites no history. Review pending
   reconciliations separately; a changed selection can make them stale.

## Initialize, migrate, inspect, or back up

1. Select the exact target. Confirm an absent path for initialization and the
   intended immutable kind. Confirm initialized private Bazaar state and a
   complete `cell.prompts.annals` selection before initialization. Do not import
   prompt contents or substitute fallback text as a runtime repair.
2. Run the authorized deterministic command:

   ```sh
   annals init [--kind general|decisions]
   annals migrate
   annals stats
   annals backup /absolute/absent-backup.db
   ```

3. Verify the command's library identity, kind, schema, statistics, or backup
   result. Migration supports schemas 3 through 6 to schema 7 transactionally,
   preserves source and corpus history, and leaves unknown historical
   instruction provenance null. Stop on unsupported state. Initialization and
   backup refuse replacement. A destructive fresh-state cutover requires its
   separately authorized installation procedure.

## Submit and apply a direct reconciliation

1. Read the target work, current corpus revision, and current instructions.
   Prepare strict reconciliation JSON using the corpus-change feature's input
   contract. Use public `cN` IDs and exact source quotations, not concept labels,
   paths, or source byte offsets.
2. Submit and inspect the proposal:

   ```sh
   annals change submit /absolute/request.json --work LABEL --base REVISION
   annals change show --work LABEL
   annals change validate --work LABEL
   ```

3. Verify the projected concepts, edges, evidence, and instruction basis.
   Submission validates but does not apply. A mechanically equal result is
   recorded without a commit. Stop on invalid selectors, missing evidence,
   graph invariants, or ambiguous pending selection.
4. Apply only when corpus application is authorized:

   ```sh
   annals change apply --work LABEL
   ```

5. Verify the applied status, new revision, and exact commit effects. Application
   checks HEAD and selected instructions in its committing transaction and
   applies the complete transition atomically. If either basis changed, stop
   and prepare under current context. No force path exists.

## Simplify or revert corpus history

1. Preview simplification when it is intended:

   ```sh
   annals shake
   ```

2. Review every proposed direct-edge removal and the exact library, HEAD, and
   instruction basis. A direct relationship can carry meaning even when a
   longer path exists. Confirm only when removing those edges is authorized.
   `--yes` supplies explicit confirmation; JSON without it returns an
   informational `confirmation_required` preview. Cancellation changes nothing.
3. Verify the new commit if a nonempty plan applied. Shake preserves reachability,
   but can change direct-neighbor counts, shared flags, and hop distances.
   Stop on a stale plan; do not force it.
4. Inspect history before an authorized revert:

   ```sh
   annals log
   annals diff FROM_REVISION TO_REVISION
   annals revert REVISION
   ```

5. Verify the new inverse commit and remaining unrelated state. Revert never
   erases history. Relevant intervening facts cause an atomic conflict; inspect
   that conflict rather than editing history.

## Prompt selection maintenance

Initialization relies on the complete Bazaar selection described by
`annals.libraries`. Use this procedure only for an authorized Annals prompt edit:

1. Publish component text with `bazaar update PROMPT_ID --file /absolute/prompt.txt`.
2. Read each returned positive version and prepare the complete selection JSON.
3. Publish it with `bazaar update cell.prompts.annals --file /absolute/selection.json`.
4. Read the selection through Bazaar's supported `get` interface and verify every
   pinned component. When Annals uses `CELL_BAZAAR_DATABASE`, use an explicit
   `bazaar --database /absolute/private/bazaar.sqlite3` prefix for each command.
5. Append the prior complete selection content to roll back an authorized edit.
   Retain migration selection version 1 and historical text versions. A text
   append alone does not select it, and later edits rewrite no frozen request.

## Stop and recover

Stop on conflicting identities, unsupported schemas, missing prepared read state,
stale reconciliation or shake context, failed invariants, or a revert conflict.
Use the owning feature and installation recovery route. Do not edit databases,
catalogs, spool receipts, or history directly. Do not overwrite backups or infer
application authority from read access. This operation authorizes only its
selected effects, not external disclosure or storage cleanup.

CLI usage recording follows `annals.libraries`; recording errors preserve results.
Feature contracts expose unspecified capacity, latency, compatibility-window,
and backup-retention promises rather than supplying additional guarantees here.
