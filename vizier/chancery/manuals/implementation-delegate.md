# Delegate an implementation

Use Vizier only after the implementation request and authority are settled.
The caller supplies exact Markdown rather than asking Vizier to infer
requirements or split one document semantically.

Before submission, read the applicable Semantics repository through its
supported interface and preserve that exact JSON inside a caller-owned
terminology Markdown document. Vizier freezes the document but never calls
Semantics itself.

```sh
vizier run submit \
  --repository /absolute/repository \
  --source EXACT_GIT_REVISION \
  --brief /private/brief.md \
  --terminology /private/terminology.md \
  --contract api=/private/api.md \
  --contract storage=/private/storage.md \
  --gate product-ci='./ci.sh' \
  --remediation-rounds 1
```

Contract units are planned separately, then assembled into work packets so
overlapping contracts need not become competing writers. Vizier performs one
independent review of that assembled plan, delegates ready packets in isolated
Git worktrees, freezes each source candidate, obtains an independent review,
integrates accepted candidates, runs the supplied gates, and obtains one final
independent review.

Reviewers route with `accepted`, `changes_requested`, or `blocked`. A change
request is valid only when its Markdown cites an existing contract or accepted
packet criterion. Remediation receives that exact report and a targeted
recheck. Advisories do not create work, and exhaustion becomes
`needs_attention`.

After interruption, inspect and resume the exact run:

```sh
vizier run show RUN_ID
vizier run resume RUN_ID
vizier run wait RUN_ID
```

Do not infer success from Nucleus state or model prose. Success is the durable
Vizier result naming the exact integrated candidate that passed all required
packet reviews, gates, and final review. Vizier leaves the caller's branch
untouched and never pushes, releases, or deploys the result.
