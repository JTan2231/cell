# Assess a Todo's current situation

A situation assessment describes what is true now for one established open
canonical `tN`. It maps accepted direction boundaries to current evidence,
constraints, dependencies, gaps, and jurisdiction. It cannot revise the todo,
choose a desired design, or authorize work.

```sh
/Users/joey/.local/bin/todo assess <TODO_ID>
/Users/joey/.local/bin/todo situation show <ASSESSMENT_ID>
```

## Frozen assessment basis

Todo freezes the current direction revision, attached concerns, working-note
cursor, accepted design if any, its own read projection, and a catalog of
relevant sources. Concrete documents receive stable source IDs. The liaison
can list, read, and search only that catalog through managed tools; it has no
shell, workspace, inherited environment, or web search.

Source reads return exact evidence references. The committed `aN` stores the
mapping from each cited source ID to its locator, frozen revision, and
observation time. Source text is untrusted evidence, not runtime instruction.

## Required result

One immutable dated assessment includes:

- subject identity and stable references;
- grounded current-state, constraint, dependency, and gap findings;
- coverage of every direction boundary;
- jurisdiction findings assigning every relevant party exactly one role of
  owner, participant, or consumer, with exactly one owner per jurisdiction;
- material user choices, evidence gaps, and jurisdiction conflicts; and
- one disposition: `ready`, `needs_user_choice`, or `inconclusive`.

`ready` means the evidence is sufficient for design reconciliation. It does
not mean implementation is approved. `needs_user_choice` records a material
value or authority decision that cannot be inferred. `inconclusive` records a
material evidence gap. Runtime or tool failure is infrastructure failure and
must not be relabeled as an inconclusive domain finding.

## Currentness and recovery

The historical `aN` is immutable and always inspectable. Changes to its frozen
bases make it stale without rewriting it. Any newer assessment for the same
umbrella makes every older one non-current. When facts or bases change, create
a new assessment rather than editing the old one.

Todo's committed assessment is authoritative; Nucleus job completion and model
prose are not. Nucleus raw output may retain source content read by the
liaison, while Todo retains exact source references and frozen provenance.
