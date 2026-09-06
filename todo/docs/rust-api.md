# Todo Rust interface

The `todo` crate owns its supported Rust boundary in `todo::api`. Import
these values and its client at a product boundary, then convert into local
application values when needed. Consumers must not copy its response envelope,
wire records, or decoder. The structs and enums are the interface definition;
there is no separate registration layer.

`Client::new` takes an explicit executable path. `with_database` selects the
existing CLI database override. `execute(&Request)` performs one command and
returns `Reply<Data>`; `execute_with_input(&Request, bytes)` additionally sends
exact standard-input bytes when the selected command accepts `-`.
`Reply::diagnostics` preserves successful-command stderr, which can describe a
durable domain result followed by a runtime problem. The client does not retry.
`ClientError::Rejected(Failure)` retains a provider domain error, separately
from process I/O, malformed JSON, or invalid envelopes.

`Request` covers concern capture and research, routing decisions, assessments,
design proposals and decisions, umbrella reads, notes and lifecycle changes,
email, initialization, and migration. It reuses the command argument types,
including distinct `ConcernId`, `RoutingProposalId`, `TodoId`,
`SituationAssessmentId`, `DesignId`, and `WorkingNoteId`. `Data` preserves the
existing untagged payloads, including pending research diagnostics and complete
assessment/design projections. `api::tools` exports the managed-tool input
values used by the provider's existing decoders. Tool proposals cannot authorize
routing or design decisions.

Use `Client::with_config` to select the existing Todo TOML configuration. Email
and research requests retain their documented external effects; constructing a
client or importing a request type does not invoke either.

`Success`, `Failure`, `Data`, and the exported records are serializable and
deserializable. The CLI emits these same provider-owned payloads and envelopes.
`decode_response::<Data>` is available for an existing caller-owned transport;
the ordinary integration should use `Client`. Existing JSON field names,
nullable/omitted fields, output versioning, errors, state transitions, and
privacy boundaries remain the CLI contract. See [CLI](cli.md).

The supported API does not expose database connections, workers, or mutable
storage operations. It invokes the same noninteractive CLI boundary, with the
same authority requirements. The crate is a source dependency; installation
continues to select the ordinary executable, not a separate runtime service or
shared-library deployment. No cross-product Rust consumer currently imports
this API in the workspace; future consumers should depend on this provider
crate rather than defining the wire interface themselves.
