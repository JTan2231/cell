# Read Conatus sources and associations

Use this capability to inspect captured wants, accepted decision projections,
their available Annals evidence and associations, or processing state. These
commands start no model and change no domain records.

```sh
/Users/joey/.local/bin/conatus want list --limit 20
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

Want wording is the supplied source assertion. A decision document is a
deterministic rendering of accepted feed fields, not original account Markdown
or retrieved conversation quotation. A decision's authority anchor remains a
reference. Acceptance does not prove enactment, progress, or current force.

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
capture and successful handoff; decision occurrence retains its supplied
precision. Annals revision times describe corpus changes.

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
