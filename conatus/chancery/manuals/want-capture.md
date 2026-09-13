# Capture a want

Use this capability when a user-authored want should be retained in its exact
source wording and later related to decisions. Do not capture assistant
suggestions or hypothetical examples as the user's wants. Capture does not
derive qualifications, priorities, deadlines, lifecycle state, or further wants.

Conatus state must already be initialized. Select it with the global
`--state-dir ABS_PATH` when using a nondefault location. The default is
`CONATUS_STATE_DIR` or `~/Library/Application Support/Conatus`.

```sh
/Users/joey/.local/bin/conatus want add 'I want more time to write.' --source 'conversation reference'
/Users/joey/.local/bin/conatus want add --file want.txt --source 'conversation reference'
/Users/joey/.local/bin/conatus want add --stdin --source 'conversation reference'
```

Supply exactly one nonblank UTF-8 text argument, file, or standard-input stream.
Conatus preserves the supplied wording and source-reference string. The
reference is opaque provenance; Conatus neither fetches it nor proves its
content. Input files are transport and remain unchanged.

A successful response is JSON `{ "ok": true, "data": ... }` identifying the
locally committed intake record. Errors use
`{ "ok": false, "error": "..." }`. `--json` is accepted but not required.
The want ID is `want-` followed by a UUIDv7. `captured_at` records local capture
in UTC Unix seconds; it is not the source's authorship time. The outgoing Annals
document is frozen with this record. Its work name is the intake ID, and the
outgoing filename adds `.md`.

Capture starts no model and changes no Annals corpus. It establishes durable
Conatus intake, not enqueue, library retention, or interpretation. A later
`conatus update` forwards pending documents through the selected Annals inbox.
Do not report an input or storage error as successful capture.

The full statement and reference remain in private local Conatus state.
Subsequent update can supply them to Annals and its configured Nucleus model
integration. Capture does not authorize publication, direct Annals edits,
installation, or schedule activation. No maximum source size, storage capacity,
or cross-release compatibility window is promised.

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.
