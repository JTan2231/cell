# Rust interface

`annals::api` exports the corpus, work, retention, reconciliation, source
activity, history, and inbox views used by the CLI. Database connections and
worker state remain private.

## Read a library

`LibraryReader` uses Annals queries and cursor rules to return provider-owned
views. It is read-only.

## Invoke the CLI

`CliClient` invokes an explicitly selected executable. A typed `Request`
produces its matching `Response`. Requests reuse the CLI argument types and
cover reads and mutations, including named libraries and stored instructions.

`CliClient::for_named_library` selects a registered library name. Expected
identity and state-root options use the same CLI checks. Input bytes require
an explicit `-` input path or `instructions set --stdin`.

Constructing a client has no effects. Each call has the effects of its selected
[CLI operation](cli.md).

## Exchange typed data

Reconciliation callers use `Reconciliation` and `parse_reconciliation`.
The separate `annals-api` crate owns accepted-account exchange and usage views.
See [account exchange](../chancery/annals/manuals/decision-account-exchange.md)
and [usage reporting](telemetry.md).
