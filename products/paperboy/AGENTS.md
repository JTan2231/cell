# Paperboy

Semantics-Project: paperboy

- Query the installed Paperboy semantic repository before analysis or changes.
  It owns terminology and history; code, tests and documentation define behavior.
- Keep Paperboy small. Scripts own collection, data windows, and rendering.
  Paperboy owns production-definition validation and the execute-and-send handoff. Email owns
  submission. Clockwork owns scheduled activation and runtime history.
- Execute literal renderer argv. Preserve successful nonempty stdout as the
  plain-text email body. Keep stderr separate. Empty stdout skips submission.
- Keep the version-one TOML production-definition file as the configuration
  authority. Apply snapshots into Clockwork activation definitions. Preserve
  enabled intent for existing production schedules and leave new schedules disabled.
- Do not add source adapters, agent prompts, a database, retained messages,
  automatic retries, or delivery reconciliation without an approved change.
- Submit committed code changes through `./ci.sh submit COMMIT` from the Cell
  root and verify the manager job outcome. Follow the root CI instructions.
