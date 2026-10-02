# Usher agent instructions

Semantics-Project: usher

- Keep Usher small, deterministic, and read-only. It reports declared Cell
  membership: product identity, Semantics participation, and Chancery presence.
- Read maintained terminology through `semantics.repository.explore`. Query
  the registered `usher` project and use Cell's vocabulary for shared terms.
  Report an unavailable project instead of inferring registration from its
  marker. Source, tests, and current contracts remain authoritative for behavior.
- Keep full behavior in the owned Chancery feature pages. Publish the overview
  and required feature dependencies with the release. Keep operating essentials
  in procedures and the README as an entry point.
- Read repository declarations as files. Recognition must not access service
  databases, call agents, execute descriptors, or operate other systems.
- Maintain the documented evidence boundary and JSON compatibility when
  changing recognition rules. No registration or installation is implied by
  a source declaration.
- Submit committed code changes through `./ci.sh submit COMMIT` from the Cell
  root and verify the manager job outcome. Follow the root CI instructions.
