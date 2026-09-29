# Change Conversations

Use this operation for an authorized change to Conversations source, tests,
protocol compatibility, typed schema, packaging, or documentation.
Read `AGENTS.md`, `semantics.repository.explore`, and the registered Conversations
repository before analysis. Read `nucleus manual` before public-contract,
compatibility, persistent-state, authentication, lifecycle, deployment, or
requester-integration changes.

Read `chancery resolve conversations.develop.change` for the required contracts.
`conversations.history.explore` owns query, normalization, and activity behavior.
`conversations.runtime` owns launch, diagnostics, readiness, and installation
guarantees. `conversations.installation.operate` owns manual installation and
recovery steps. The overview connects those features and operating routes.

## Prepare the smallest change

1. Select the affected feature and intended compatible outcome.
2. Consult current official Codex App Server documentation before changing protocol names, parameters, source kinds, pagination, or compatibility assumptions.
3. Change the smallest owning code and its authoritative feature, operation, or overview in the same candidate.
4. Keep older documentation as navigation to those contracts instead of a second explanation.

App Server remains the discovery and storage-compatibility authority.
Stop when required data is unavailable through its documented API, a schema
change would silently break a consumer, or the change needs task mutation,
persistent indexing, raw storage access, or a broader product boundary.
Do not infer activity from unrelated processes or expose reasoning and tool
payloads. Use synthetic fixtures; no real transcript belongs in tests or bundles.

## Validate the affected boundary

Use synthetic fake-App-Server fixtures as appropriate for initialization,
active and archived pagination, all-source enumeration, root/subagent and exec
filters, state-database-only defaults, explicit refresh, turn pagination, the
narrow legacy fallback, normalized content, stable references, and copied-item
deduplication. Preserve complete-read failure rather than reporting partial
normal success.

Exact-summary changes must prove canonical host matching, active and archived
metadata lookup, and no turn reads. Process changes must prove that a wrapper's
persistent descendant cannot outlive its private launch group and that unrelated
processes remain outside cleanup. Packaging changes must retain content identity,
tamper rejection, prior selectors, foreign-selector protection, and post-switch
restoration.

Validate the complete candidate bundle with `chancery validate BUNDLE`.
Inspect its product overview, each ordinary show page, and required resolution
closure using an isolated registry. Keep unsupported, unspecified, and upstream
reliance gaps visible. Source checks do not establish release publication,
installed selection, live runtime compatibility, or a retrieved history result.

## Deliver and verify

Commit the approved change and submit it with `./ci.sh submit COMMIT` from the
Cell root. The installed manager integrates, validates, attempts bounded repairs,
deploys, and emails the outcome. Verify the retained job outcome before treating
that delivery as complete. Focused tests support development and do not replace
the manager job. CI submission includes its deployment and outcome email.

`release.sh` commits, tags, and pushes. Release publication and installed
manual deployment outside the submitted manager job require their separate
applicable authority. Use `conversations.installation.operate` for an authorized
manual deployment or recovery. Rebuild affected embedded consumers when library
behavior changed; replacing the CLI alone does not update them.

## Privacy and command usage

Keep fixtures and documentation synthetic. Avoid private paths, transcript text,
and credentials in examples and provider bundles. This operation grants no
history retrieval, task mutation, private-storage, authentication, unrelated
process, or transcript-disclosure authority.

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.
