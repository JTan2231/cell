# Job Packets

Job Packets prepares up to three previously unsent opportunities from Cast.
Each packet contains a brief paragraph and a tailored resume PDF. Two Nucleus
jobs use `gpt-5.6-sol` with `max` reasoning: one assesses and briefs the role,
and the second writes only the Jackson work-experience bullets when the role
is worth pursuing. Both can investigate the same captured CRM career library.

Everything outside the Jackson bullet span in the original resume's LaTeX
source is fixed, including identity, dates, employers, other experience,
education, projects, skills and layout. Model output is plain bullet text,
escaped before insertion. Generated PDFs must fit the original one-page
layout. The original template and generated materials stay in private state.

This is a source-built prototype. **The stored delivery setting is 09:00 in
America/Chicago; no recurring schedule is installed or enabled.** There is no
candidate or token budget. External account limits, execution timeouts and
source/rendering constraints still apply.

```sh
./ci.sh job-packets
target/debug/job-packets init --resume /absolute/path/to/original-resume.tex
target/debug/job-packets prepare CAST_JOB_ID
target/debug/job-packets prepare-daily
target/debug/job-packets preview 2026-09-07
target/debug/job-packets status
```

`preview` freezes one through three ready packets into a dated edition,
including the exact email text and copied PDF attachments. It reserves those
opportunities so a later edition cannot select them again. It sends nothing.
Only after the edition is authorized for delivery:

```sh
target/debug/job-packets send 2026-09-07
```

Sending requires the Email implementation that supports repeated `--attach`
arguments; an older installed Email command cannot send these editions.
Email's recipient remains the fixed personal inbox. Acceptance is retained
separately from final inbox receipt. An uncertain send is held for inspection
and is not automatically retried.

For an authorized test email using retained packets, create a separate ad hoc
occurrence. This path does not refresh job sources, invoke Cast or CRM, run a
model, reserve ordinary packets, or mark them sent:

```sh
target/debug/job-packets preview 2026-09-07 --ad-hoc sample-1 --packet PACKET_ID
target/debug/job-packets send 2026-09-07 --ad-hoc sample-1 \
  --email-executable /absolute/private/email-candidate-wrapper
```

The preview prints a `[TEST]` edition and freezes the selected retained
paragraphs and PDFs under private `ad-hoc/sample-1/` state. Sending requires
explicit test-send authority and retains its own receipt. Reusing the same
ad hoc ID after acceptance does not send again; an uncertain attempt remains
held. The optional Email executable override affects this send only. A tested
source Email binary can use a temporary copy of Email's credential wrapper;
no production Email deployment is necessary for the test.
Preview can also accept `--brief-overrides /absolute/reviewed-paragraphs.json`,
a mapping from selected packet IDs to reviewed paragraphs. Overrides belong
only to the test occurrence; the accepted original briefs stay unchanged.

Cast owns discovery, CRM owns the career library, Nucleus owns constrained
execution, and Job Packets owns preparation, accepted materials, editions and
send history. No direct CRM or Cast database access is used. Similar roles
without a shared canonical identifier are not guaranteed to deduplicate.

The full [prototype CLI and recovery contract](chancery/manuals/packet-prepare.md)
describes private state, source limitations and recovery. The source provider
bundle is validated by CI; it is not an installed Chancery selector. Until a
separate semantic repository is registered, this product uses Cell terminology.
