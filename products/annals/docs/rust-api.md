# Rust interface

The owning feature contracts publish provider-owned Rust interfaces with their
selection, effects, data, errors, and compatibility boundaries:

| Interface | Authoritative explanation |
| --- | --- |
| `annals::api`, `LibraryReader`, `CliClient`, `CliClient::for_named_library`, `Request`, and `Response` | [Libraries and clients](../chancery/annals/manuals/libraries.md#read-access-and-rust-clients) |
| Read views and query/cursor behavior | [Corpus reading](../chancery/annals/manuals/corpus-explore.md#replay-and-rust-access) |
| `Reconciliation` and `parse_reconciliation` | [Corpus changes](../chancery/annals/manuals/corpus-change.md) |
| `annals-api` accepted-document types and client | [Document exchange](../chancery/annals/manuals/decision-account-exchange.md) |
| `annals-api` usage views | [Annals Usage](../chancery/annals-usage/manuals/consumption-inspect.md) |

Read installed features with `chancery show ID`. Use `chancery resolve ID` for
required contracts and gaps. Use [command navigation](cli.md) to identify the
operation represented by a typed client request.
