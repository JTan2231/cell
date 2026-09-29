# Read Conatus sources and associations

Use this capability to inspect captured wants, accepted decision documents,
their available Annals evidence and associations, or processing state. These
commands start no model and change no domain records.

```sh
/Users/joey/.local/bin/conatus want list --limit 20
/Users/joey/.local/bin/conatus want list --archived
/Users/joey/.local/bin/conatus want list --all
/Users/joey/.local/bin/conatus want show ID
/Users/joey/.local/bin/conatus decision list --limit 20
/Users/joey/.local/bin/conatus decision show ID
/Users/joey/.local/bin/conatus graph
/Users/joey/.local/bin/conatus history --limit 20
/Users/joey/.local/bin/conatus status
/Users/joey/.local/bin/conatus instructions show
```

Use global `--state-dir ABS_PATH` to select initialized local state. The default
is `CONATUS_STATE_DIR` or `~/Library/Application Support/Conatus`. Output is JSON
`{ "ok": true, "data": ... }` or `{ "ok": false, "error": "..." }`;
`--json` is optional. Intake lists default to 20 and accept 1 through 100.
They select local intake by source kind. A show selects one exact intake ID and
separates its `record`, `retention`, `interpretation`, and `associations`. Each
Annals read reports available data or its own error.

Want lists select active wants by default. `--archived` selects only archived
wants; `--all` selects both states. These options conflict. Selection precedes
the limit and `has_more` calculation. Want records contain `state`, either
`active` or `archived`; decision records have no lifecycle state. `want show`
reads a want in either state. Status adds `local.wants.active` and
`local.wants.archived`. Existing intake and handoff counts include both states.

Archive state is local to Conatus. It never propagates to Annals or related
records. Archived source content remains unchanged while processing receipts
and Annals associations can evolve. Only a direct user request can change the
state through `want archive ID` or `want unarchive ID`. These reads do not
authorize either transition.

Want wording is the supplied source assertion. New decision intake preserves
the complete unchanged document from Annals, its original feed event, filename,
digest, acceptance time and transport identities. Earlier deterministic
structured projections remain historical sources; do not invent missing context
or treat them as full retrieved conversations. Acceptance does not prove
enactment, progress or current force.

A record contains `id`, `kind`, `source`, `wording`, `source_data`, `work_name`,
`captured_at`, `queued_at`, `receipt` and `error`. `wording` is exact want text or
the complete supplied decision document. `source_data` preserves the source
reference or original typed event. A nullable handoff time or receipt describes
enqueue, not successful interpretation. Work name is the intake ID; outgoing
filename adds `.md`.

`related_records` maps returned associations to captured wants and decisions,
with exact wording, reference, direction and direct or path relationship. It
includes only intake records grounded in the returned graph. The association
view's completeness flags apply to this projection.

Conatus IDs distinguish local intake. Want IDs use `want-` plus UUIDv7; decision
IDs use `decision-` plus SHA-256 of the source library ID, colon, and event ID.
Annals works, concepts, revisions, feed events and account IDs retain separate
identities. Concept labels are navigation text. The graph connects concepts
grounded by exact source evidence, not intake records directly.

The library instructions give parent connections their interpretation: the
child appears to serve the parent want. Multiple parents and unassociated
decisions are permitted. A model-created concept is not a newly captured want.
An edge or path does not establish measured progress or a formal want lifecycle.

Status distinguishes counts of local wants and decisions and counts of pending
or queued handoffs. Its cursor describes the consumed decision-feed prefix,
not interpreted coverage. Annals inbox state or its read error is separately
reported. A partially failed update retains its report before returning an
error; status exposes it even when Annals is unavailable. `captured_at` and
`queued_at` are UTC Unix-second times of local
capture and successful handoff. Feed `accepted_at` describes Annals document
acceptance, not decision occurrence. Historical source occurrence retains its
supplied precision. Annals revision times describe corpus changes.

## Completeness and freshness

Graph and history preserve the selected Annals interfaces' bounds and identity
semantics. Graph and association views return at most 200 concepts. Each concept
preview includes at most 20 parents, 20 children, and 20 evidence entries. Check
`concepts_complete`, `relationships_complete`, and `evidence_complete` before
treating the returned view as complete for its Annals revision.

Local and Annals reads do not form one atomic snapshot. A recent
corpus revision need not include all queued inputs. Missing or unavailable
Annals state does not erase local capture or prove that interpretation failed.

Responses can contain complete private statements, complete documents, source
references and evidence. Keep them inside the local product boundary unless
disclosure is separately requested. No remote sharing, storage-capacity,
latency, or cross-release compatibility guarantee is provided.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.

## Related contracts

Read `conatus.processing` for intake and interpretation behavior,
`conatus.want.lifecycle` for explicit state changes, and
`conatus.update.operate` for processing recovery. These references do not grant
mutation authority.
