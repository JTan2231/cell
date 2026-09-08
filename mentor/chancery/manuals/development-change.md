# Change Mentor email practice software

Use this operation for a scoped change to Mentor code, corpus format, state,
invocation policy, public commands, installation, or product contracts. It does
not run the practice loop, install a release, or change provider accounts.

Read `mentor/AGENTS.md` and the affected service and installation contracts.
Use Mentor's registered Semantics repository when it exists; until then use
the explicit Cell fallback:

```sh
semantics repository show cell
nucleus manual
```

Read `nucleus.requester.integrate` for changes to grading invocation, state or
recovery. Read the selected Email and Clockwork contracts when their boundary
is affected. Provider-owned Rust types and supported clients are the integration
surface. Do not copy provider wire structs, read private provider databases,
copy their credentials, or add direct-Codex or direct-Resend fallbacks.

Keep the product small. It reserves one authored problem, receives a complete
answer and sends an independent qualitative critique. It owns no prior-answer
archive, score history, continuing tutor conversation or configurable outbound
recipient. Broader product work requires an explicit scope and contract change.

Preserve these boundaries in code and documentation:

- Assignment reservation is distinct from accepted email submission. A stable
  problem ID and frozen corpus digest survive later corpus refreshes.
- Incoming provider IDs deduplicate mail; RFC Message-IDs thread responses.
  Sender comparison and a random route token do not establish authentication.
- An exact Nucleus request is persisted before admission. Rediscovery checks
  its identity; terminal failure does not create an automatic new attempt.
- A frozen email payload/key is reused only within the bounded retry window.
  Unknown submission does not become assumed failure or a new send identity.
- Temporary answer/request/critique content is cleared as work advances, on
  accepted response submission, or when a tick observes expiry. Retained
  metadata and external-provider records have different lifetimes.
- Nucleus gets no filesystem, execution, web, dynamic-tool or mail credential
  authority. Status, logs and errors expose no answer or critique content.
- Schema migration requires drained work and a content-free private backup.
  Initialized deployment and recovery use owned maintenance, with the worker
  disabled and pause settings preserved.

Implement the smallest change in its owning product. Update the product's
service, installation and Chancery declarations in the same change. Update
`nucleus/docs/operator-manual.md` when shared operational facts change. New
incompatible corpus or input meanings need a new version; old assignments
must remain interpretable from their retained corpus.

Use authored corpus data and synthetic answers in source examples. Do not put
live incoming mail, credentials, model results or Nucleus records in source,
fixtures, documentation or provider bundles.

`mentor/ci.sh` is the product validation entry point and `./ci.sh` is the root
gate. Run only the checks permitted by the current task. If tests, builds,
formatting or validation are deferred or forbidden, report that clearly; do
not call the source change tested or ready for deployment. Publication,
installation, schedule enable, model execution and real email are separate
effects that require applicable authority. A source Chancery bundle becomes
installed discovery only when it is staged and selected with a release.

Stop the change rather than weakening ownership, retaining an undeclared answer
archive, deleting provider records, replacing unknown outcomes with guesses,
or making an unsupported state format appear compatible. Preserve unresolved
maintenance and recovery evidence. Development success is a synchronized,
scoped implementation with its actual evidence and remaining limits stated.
