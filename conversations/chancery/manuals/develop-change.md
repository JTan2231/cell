# Change Conversations

Read `AGENTS.md` and the exact architecture, CLI, installation, and Chancery
contract affected by the change. Consult the current official Codex App Server
manual before altering protocol names, parameters, source kinds, pagination,
or compatibility behavior.

The non-negotiable ownership line is simple: App Server owns discovery and
storage compatibility; Conversations owns normalization and query semantics.
Do not open Codex JSONL or SQLite state, infer activity from processes, add a
daemon or index, or expose reasoning and tool payloads as a shortcut.

Use synthetic fake-App-Server fixtures to prove initialization, active and
archived pagination, all-source enumeration, root/subagent filtering,
state-database-only defaults, explicit refresh, turn pagination, legacy
fallback, normalized content, stable references, and fork deduplication as
appropriate. An exact-summary seam must also prove canonical host matching,
active-and-archived metadata lookup, and the absence of turn reads.
Process-lifecycle changes must also prove that a wrapper's
persistent descendant cannot outlive the short-lived client and that cleanup
is scoped to the launch's private process group. Finish with `./ci.sh` inside
the product's 60-second deadline.

`release.sh` commits, tags, and pushes. The macOS deployer changes the installed
binary and Chancery provider selectors. Neither effect follows from this
development contract alone.
