# Platter

Platter prepares private job briefs and Jackson-only tailored resumes from
Cast opportunities and captured CRM career entries. Nucleus owns constrained
model execution; Platter owns its retained content and delivery decisions.

The private SQLite library holds jobs with explicit eligibility, preparation
runs, immutable content artifacts, editions, ordered attachment references,
configuration and maintenance holds. A packet is a run and its artifacts.
There is no separate packet table, test-edition type or tool-receipt ledger.
Artifacts reference the run that produced them. Imported templates have no run.

```sh
platter init --resume /absolute/original-resume.tex
platter prepare CAST_JOB_ID
platter prepare-daily
platter preview 2026-09-07
platter status
platter eligibility CAST_JOB_ID false
platter export ARTIFACT_ID /absolute/chosen/resume.pdf
```

Normal preview freezes an exact message and sets the selected jobs ineligible
in the same transaction. Eligibility is an explicit field, independent of
whether delivery records exist. Change it deliberately with `eligibility`.
Retained-material preview uses the same edition model and leaves eligibility
alone:

```sh
platter preview 2026-09-07 --ad-hoc another-edition --packet PACKET_ID
```

Only after the relevant send is authorized:

```sh
platter send 2026-09-07
platter send 2026-09-07 --ad-hoc another-edition
```

Email's `--payload-stdin` extension receives attachment bytes directly, with no
exported attachment files. Accepted editions are not resent; uncertain sends
remain held. Email's fixed recipient and credential ownership are unchanged.

Both model stages use `gpt-5.6-sol` with max effort. Briefs contain Why it works,
Role and optional Culture, at most 90 words total. Unsupported culture is
omitted without additional research. The displayed recommendation has no
caveats or hedging; the pursuit assessment remains private. The resume model
can author only Jackson bullet text. Every other original source byte stays
fixed, and PDFs must pass one-page rendering checks.

Fresh state uses `~/.local/share/platter/packets.sqlite3`. A sole predecessor
`~/.local/share/job-packets` root remains in place. Two roots are ambiguous;
independent custom live libraries are unsupported. Schema-one file-backed
state requires the maintained migration before ordinary use. A schema-two
SQLite backup contains the full retained library.

Rendering still requires Tectonic, Python and pypdf. Disposable working files
and redirected caches stay inside Platter's root and are removed after use.
Explicit exports are user-owned copies. Nucleus retains its own runtime state.

The stored 09:00 America/Chicago setting creates no recurring schedule.
There is no candidate or token budget; external and rendering limits remain.

See the [operating contract](chancery/manuals/packet-prepare.md),
[data model](docs/data-model.md), and
[installation and migration contract](chancery/manuals/install-operate.md).
This source change does not install commands or migrate the live library.
Until a separate semantic repository is registered, Platter uses Cell terminology.
