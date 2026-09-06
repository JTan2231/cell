# Geste Rust interface

The `geste` crate owns its supported Rust boundary in `geste::api`. Import
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

`Request` covers capture creation/revision, episode reads, search, report,
graph, initialization, and doctor. `Capture` and its nested source, settlement,
and outcome values are the accepted document types. `decode_capture` applies
the same strict JSON, size, reference, and authority validation as CLI input.
Serialize a `Capture`, then pass those exact bytes to a create/revise request
whose input is `-`; the submitted digest describes those bytes. `Data` and the
revision/report/graph values preserve source boundaries and warnings.

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
