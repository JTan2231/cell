# Vizier semantic seed

This definition list describes Vizier's initial public behavior. It is input
for an explicit `semantics repository seed-markdown` operation after project
registration; the file alone does not register or seed the project.

**Implementation run**
: One finite Vizier coordination record bound to an exact implementation brief, terminology snapshot, ordered contract set, Git source identity, gate set, and remediation policy.

**Contract unit**
: One caller-enumerated opaque Markdown requirement boundary. It is the unit of planning, not necessarily the unit of implementation.

**Terminology snapshot**
: Exact Markdown wrapping a caller-frozen Semantics repository snapshot. It guides terminology, creates no requirements, and is never fetched by Vizier at runtime.

**Unit plan**
: Opaque Markdown produced for one contract unit against the complete frozen run inputs and source basis.

**Delegation plan**
: The accepted assembled account of implementation work together with the mechanical work-packet graph.

**Work packet**
: One implementation assignment linked mechanically to one or more contract units and dependency packets, with its meaning retained in opaque Markdown.

**Candidate**
: An exact frozen Git source identity produced from an exact base by one implementation or integration attempt. A worktree or handoff message is not a candidate.

**Review subject**
: The exact immutable object reviewed at one stage: the assembled delegation and plan revision for plan review, or a Git candidate for packet and integrated review. A successor and its targeted recheck preserve that stage's subject type.

**Review disposition**
: The mechanical routing value `accepted`, `changes_requested`, or `blocked` bound to one exact review subject; all reasons and findings remain Markdown.

**Independent review**
: Review by an invocation that did not implement or integrate the edits it may accept.

**Targeted recheck**
: The bounded review after remediation of a successor of the same review-subject type, limited to the cited finding, changed material, and affected criteria or seams rather than another broad audit.

**Integrated candidate**
: The exact Git candidate combining all accepted packet candidates and serving as the subject of configured gates and one final independent review.

**Gate**
: One caller-named command whose recorded result is evidence about the exact integrated candidate. It is not a release, deployment, or domain-success authority by itself.

**Needs attention**
: A terminal run condition requiring caller authority or intervention because bounded remediation, evidence, scope, or safe recovery is exhausted.

**Vizier domain success**
: The durable condition in which the accepted delegation exists, every required packet candidate was independently accepted, the exact integrated candidate passed every configured gate, and its independent final review accepted it.
