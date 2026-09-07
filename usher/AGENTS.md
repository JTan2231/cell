# Usher agent instructions

Semantics-Project: usher

- Keep Usher small, deterministic, and read-only. Its domain is declared Cell
  membership: product identity, Semantics participation, and Chancery presence.
- Read maintained terminology through `semantics.repository.explore`. Until
  the declared `usher` project is registered, report that gap and use Cell's
  registered vocabulary for cross-product terms. Source, tests, and these
  contracts remain authoritative for behavior.
- Read repository declarations as files; keep service databases, agent calls,
  descriptor execution, and other inter-system operations outside recognition.
- Maintain the documented evidence boundary and JSON compatibility when
  changing recognition rules. No registration or installation is implied by
  a source declaration.
- Every code change must leave the root `./ci.sh` green.
