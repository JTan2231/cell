# Product and data boundaries

Conatus records wants with their exact source wording and source reference. It
consumes newly accepted documents from the selected Annals decisions library,
then asks a separate general Annals library to organize them under Conatus'
stored instructions.

## Records and authority

Conatus intake stores a local identity, source kind, exact supplied want text or
accepted document data, source reference, capture time, the frozen outgoing
document, and handoff correlation. A captured want is an assertion supplied by
the caller. Capture does not ask a model to extract conditions or invent wants.
The source reference is opaque text; Conatus does not fetch or verify it.

Decision intake preserves the source library, feed event, document key, source
filename, content digest, acceptance time, and complete unchanged document.
The outgoing source is exactly that text; Conatus does not reconstruct an
account or require source metadata. Its library agent interprets connections
under the stored instructions. Acceptance does not prove that a decision was
enacted or remains in force.

Conatus stores a feed event and the corresponding opaque cursor advancement in
one local transaction. It forwards captured records afterward. The cursor marks
the consumed source-feed prefix, not successful Annals interpretation.

Annals retains immutable works and owns their content identities, instructions,
concepts, exact quotation evidence, parent edges, reconciliations, and revisions.
The graph connects concepts grounded in works. It does not connect intake rows
directly. Labels are navigation text; concept IDs identify graph records.

[The bundled librarian instructions](../librarian.md) define a child-to-parent relationship as
the child appearing to serve the parent want. Several parents and unassociated
decisions are allowed. Source wants constrain the interpretation: new desires
and qualifications must not be invented. A model-created concept is not another
captured want. Annals enforces its structural and quotation rules; service to a
want remains an interpretation governed by the instruction document.

Neither a direct edge nor a path establishes measured progress. No completion,
priority, deadline, or formal want lifecycle is derived. Later corrections can
be captured as further source statements; retained wording remains unchanged.

## Processing

1. Capture a want locally, or consume new accepted decision-feed events.
2. Enqueue each frozen source document in the Conatus Annals inbox.
3. Run that inbox. Annals retains the source before its model integration and
   automatically applies valid material results under the selected instructions.
4. Read Annals' work, delivery, and reconciliation records when presenting the
   input's processing outcome.

Captured means durable Conatus intake. Queued means an Annals spool envelope was
accepted. Retained means an Annals work exists. Interpreted means Annals recorded
an interpretation result. A no-change interpretation need not advance the
corpus revision. A successful enqueue or an Annals process exit does not prove
interpretation.

New sources go to the inbox without a preceding `work add`. Annals recognizes
duplicate retained bytes without another inbox examination. Explicit
re-examination uses the integration interface instead.

An unavailable Annals or Nucleus path can leave inputs captured or queued.
Subsequent updates resume ordinary intake and dispatch; failed model attempts
use an explicit bounded inbox retry. Conatus and Annals do not share a
transaction, and generic inbox enqueue does not promise producer-key
idempotency. An uncertain handoff can produce a duplicate delivery. The source
work still has Annals' byte identity; inspect the durable work and interpretation
result before deciding what remains.

Instruction replacement appends a library selection revision. It does not
rewrite sources or automatically reinterpret history. Annals freezes the
instruction and corpus revisions at admission and rejects stale material
application. A recorded domain result survives a later execution failure.
Conatus does not automatically simplify the graph with transitive reduction.

## Operation and privacy

Initialization pins the named general library and decisions-library identities,
installs the bundled instructions, and takes the current decision-feed watermark
as the baseline. It does not import earlier decisions, start a model, or enable
a schedule. Each update consumes new events through a newly selected watermark.

Repeating `init` can replace the Annals executable after both pinned library
identities pass verification. It preserves the configured libraries, cursor,
records, instructions, and pause state.

Conatus invokes Annals through its public clients. Annals remains the Nucleus
requester. Conatus introduces no separate model toolset, authentication
authority, or agent runtime. Clockwork activation is separately enabled; Conatus
serializes its own update paths. Pause gates subsequent updates; an active
update finishes, and explicit retry or re-examination remains available. It does
not disable a Clockwork binding or cancel an already admitted Annals model job.

Source wording, full documents, references, outgoing documents, and receipts are
private local product state. Integration can send a retained document and
relevant corpus context through Annals to Nucleus and its configured model.
These files and databases have no automatic pruning or remote sharing surface.

The supported product scope is capture, association, inspection, and explicit
recovery. It does not include task planning, automatic conversation scanning for
wants, historical feed import, progress measurement, notifications, or public
sharing. Current live readiness and successful deployment require separate
operational evidence.
