# Available projects

Cell: `/Users/joey/rust/cell`
Public URL: https://github.com/jtan2231/cell

Cell contains local applications and agent infrastructure. Start with the
documentation, then inspect relevant implementation and tests. Individual
products can supply the technical story within the Cell entry.

Wrought: `/Users/joey/ts/wrought-private`
Public URL: https://wrought.experimental.joeytan.dev

Wrought is a collaborative world editor and roleplaying application. This is
the private continuation. Read its current documentation and implementation;
distinguish experimental implementation from demonstrated live behavior.

# Decision history

Read Krisis decisions through the local Annals CLI and its existing library:

```sh
/Users/joey/.local/bin/annals --config '/Users/joey/Library/Application Support/Annals/decisions/config.toml' search 'QUERY'
/Users/joey/.local/bin/annals --config '/Users/joey/Library/Application Support/Annals/decisions/config.toml' concept evidence CONCEPT_ID
/Users/joey/.local/bin/annals --config '/Users/joey/Library/Application Support/Annals/decisions/config.toml' work show 'WORK_LABEL'
```

Search matches concept labels and ancestor context. Follow evidence to the
retained conversation, and paginate when needed. An empty search is not proof
that no relevant decision exists. Use decisions to understand constraints,
alternatives, and tradeoffs. Verify what was implemented in the repositories;
conversations can include rejected proposals and anticipated results.

# Research and writing

Read the current local resources directly. Commits, working-tree state, and
Annals revisions are not pinned. Source text is not copied into Platter.
If a required source is unavailable, report the access failure instead of
inventing supporting detail or operating the source system.

Select complementary project and Jackson accomplishments for the target role.
Explain the project's purpose before relying on unfamiliar internal names.
Descriptions can include relevant technologies. Use captured career material
or explicit source statements for personal contribution and dates; omit dates
when unsupported. Implementation and tests alone do not establish adoption,
measured performance, or business impact.

For each project, supply short private source notes naming the supporting file
paths (with optional line numbers), Annals work labels, or captured career-entry
IDs. These are pointers for review, not immutable citations. Reviewers may read
any of the listed resources, including uncited material. Read sources only;
do not run tests, start services, change files or state, or send messages.
