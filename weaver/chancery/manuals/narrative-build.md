# Build a public-facing narrative

Weaver compiles authored narrative inputs into five current Markdown outputs.
It is a durable content workflow, not a job-search, application, browser, or
publication system.

The selected repository must contain one narrative with `basis.md` and
`brief.md`, plus the shared and stage-specific prompt contract under
`workflow/narrative/`. The basis identifies the canonical original sources.
Weaver rejects unsafe narrative names, missing authored inputs, symlinked
authority paths, and malformed output trees before admitting work.

## Submit and observe

```sh
/Users/joey/.local/bin/weaver \
  --repo <NARRATIVE_REPOSITORY> \
  submit <NARRATIVE>

/Users/joey/.local/bin/weaver \
  --repo <NARRATIVE_REPOSITORY> \
  wait <RUN_ID>
```

`submit` atomically records the sole current run before it starts a detached
one-shot Weaver worker. It prints the run ID and exits. Strict Nucleus
readiness is checked by the worker before any prior generated output is
cleared.

The worker runs five stages in order: stories, themes, composition, editorial
review, and finalization. Every stage is a separate Nucleus job. Immediately
before a stage, Weaver reads only that stage's selected authored and generated
inputs through the interactive caller's process lineage and embeds their
labeled contents in the exact request. Nucleus-launched Codex receives no
repository working directory, local execution, web search, launch context, or
dynamic toolset.

Weaver validates each nonempty Markdown result and atomically writes its
`output.md`. If a later stage fails, earlier outputs from the new run remain and
uncompleted later outputs remain absent. Weaver does not restore the old five
outputs or invent another model attempt.

## Prove the result

```sh
/Users/joey/.local/bin/weaver \
  --repo <NARRATIVE_REPOSITORY> \
  status <RUN_ID>

/Users/joey/.local/bin/weaver \
  --repo <NARRATIVE_REPOSITORY> \
  check <NARRATIVE>
```

`check` is deterministic and invokes no model. It validates all five files,
story anchors and links, the exact review verdict, and final-output consistency.
`PASS` and `REVISE` are mechanically valid and exit zero. `BLOCKED` is a valid
diagnostic but not publishable narrative and exits 3.

Nucleus completion alone does not prove a build. Weaver must persist every
required output and the repository must pass mechanical validation. Generated
text remains current working material rather than factual authority.

## Recovery and privacy

A nonterminal `wait <RUN_ID>` starts a replacement detached worker immediately
and periodically while it observes the run. That worker reads the exact active
request saved in `current.json` and rediscovers its deterministic Nucleus job.
This is the supported recovery after a worker crash, logout, or machine
restart. A Nucleus restart instead makes an unfinished attempt `lost`, which is
terminal for the Weaver run.

During an active stage, Weaver private state contains the complete request and
embedded input bytes. Nucleus retains the complete request and raw protocol
exchange in its private database. Protect both state roots. Weaver creates no
completed-run archive and never publishes, sends, uploads, applies for a job,
or edits a public profile.
