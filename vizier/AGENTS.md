# Vizier agent instructions

Semantics-Project: vizier

- Keep Vizier a short-lived local CLI and keep its workflow finite. Do not turn it into a daemon, general scheduler, workflow engine, requirements parser, deployment orchestrator, or release gate.
- The Vizier semantic repository owns maintained terminology and its append-only history. Vizier source, tests, migrations, and public product documentation remain authoritative for runtime behavior. Until this project is registered and seeded, use the enclosing Cell semantic repository; afterward query the Vizier repository before analysis, review, documentation, or implementation. Never edit Semantics state directly.
- Preserve every caller-supplied brief, contract unit, terminology snapshot, plan, handoff, review, rationale, and remediation instruction as exact opaque Markdown. Structure only workflow mechanics such as identities, digests, dependency edges, dispositions, jobs, workspaces, candidates, and gates.
- Implementors never accept their own edits. Permit one broad plan review, one broad review per packet candidate, and one broad integrated review. Remediation receives only a targeted recheck of the cited finding and affected seams. A blocking finding must cite an existing contract or accepted packet criterion; route unstated requirements, authority ambiguity, and wider scope to the caller.
- Vizier owns workflow state, candidate identity, retries, recovery, gates, and domain success. Nucleus owns bounded job execution only. Never treat model prose or Nucleus completion as Vizier success, and do not add a direct Codex fallback.
- A Vizier run never moves the caller's branch, pushes, deploys, releases, amends requirements, or mutates Pratica or Semantics.
- Keep public documentation, Chancery contracts, persistence behavior, and the Nucleus operator manual aligned with behavior changes.
- Every code change must leave `./ci.sh` green.
