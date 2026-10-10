# Paperboy

Paperboy provides production runs from TOML production definitions. Each
definition selects a renderer, optional email subject, and schedule. A run
passes successful nonempty stdout unchanged to Email as a plain-text body.
The renderer owns collection, data windows, content, and source integration.

Use `paperboy.report.send` for the production-definition format, renderer contract, manual
execution, email result, privacy, and limits. Use `paperboy.install.operate`
for installation, initialization, diagnostics, schedule snapshots, and explicit
activation through Clockwork.

Paperboy retains production definitions and product logs. Clockwork retains
immutable activation definitions, binding selection, incidents, and run history. Email owns
fixed-recipient submission and reports provider acceptance. Paperboy retains
no email body, report history, or retry queue.

Scheduled logs can retain execution result metadata, including an accepted
provider message ID. They provide no retained-payload recovery interface.
