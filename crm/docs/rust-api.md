# CRM Rust interface

The `crm` crate defines its supported Rust interface in `crm::api`. Import
its types and client, then convert results to local application types as needed.
Consumers must not copy its response envelope,
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

`Request` covers case creation, intake, case/search reads, update reads, wait,
resume, retry, initialization, explicit migration, doctor, and profile
creation/list/show/replacement. Profile bodies are exact Markdown. Profile
updates replace content in the same way as the CLI. `Data` distinguishes each
supported success payload. `CaseRevision`, `CaseListItem`, `SearchResult`, `UpdateView`,
and `Failure::context` retain advisory text and attention; a local conversion
must preserve their non-blocking visibility. `RevisionProposal` is the exact
managed-tool submission shape. The hidden worker is not a public command.

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
