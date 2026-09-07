# Usher agent instructions

Semantics-Project: usher

- Keep Usher small, deterministic, and read-only. It reports declared Cell
  membership: product identity, Semantics participation, and Chancery presence.
- Read maintained terminology through `semantics.repository.explore`. Until
  the declared `usher` project is registered, report that gap and use Cell's
  registered vocabulary for cross-product terms. Source, tests, and these
  contracts remain authoritative for behavior.
- Read repository declarations as files. Recognition must not access service
  databases, call agents, execute descriptors, or operate other systems.
- Maintain the documented evidence boundary and JSON compatibility when
  changing recognition rules. No registration or installation is implied by
  a source declaration.
- Every code change must leave the root `./ci.sh` green.
