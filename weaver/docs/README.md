# Weaver documentation

- [Architecture](architecture.md) defines Weaver's authority, execution,
  current-state, recovery, and maintenance boundaries.
- [CLI contract](cli.md) documents command selection and outcome semantics.
- [macOS user installation](system-installation.md) documents deployment,
  detached activation, prototype migration, and recovery.
- The product-owned [Chancery provider manifest](../chancery/provider.json)
  catalogs narrative-build, workflow-operation, and development-change entries
  with detailed manuals. It is discovery documentation, not a runtime
  dependency or an expansion of Weaver's authority.

Use `chancery list` to find capabilities and `chancery show` to read each
applicable contract. Select one entry, then run `chancery resolve <ENTRY_ID>`
to read its provider scope, boundary claims, documentation dependencies, basis,
and gaps. This result does not establish readiness, authorization, or domain
success.

The narrative repository defines the editorial vocabulary and authored input
contract. Weaver implements its complete build request. These documents cover
Weaver's operation. The narrative project owns its career-fact and disclosure
rules.
