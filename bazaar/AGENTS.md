# Bazaar

Semantics-Project: bazaar

- Keep Bazaar a store of opaque strings keyed by ID and immutable version.
- Use Cell terminology until a Bazaar Semantics repository is registered.
- Callers own content formats, templates, configuration interpretation, and
  agent execution. Keep these concerns outside Bazaar.
- Preserve every committed version. Updates append complete content, including
  when the content matches an earlier version.
- Keep the Rust API in process. Read operations must not create or repair state.
- Submit committed code changes through `./ci.sh submit COMMIT` from the Cell
  root and verify the manager job outcome. CI includes deployment. Publication,
  caller migration, and live content updates remain separate operations.
