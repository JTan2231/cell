# Agent instructions

- Keep changes simple; do not overcomplicate or overarchitect.
- Classify work by its intended durable outcome. Todo owns retained actionable
  follow-up, Annals owns retained evidence-grounded source knowledge, and
  Nucleus owns shared agent execution and requester integration.
- Before changing the public contract, harness compatibility, persistent state,
  authentication or service lifecycle, deployment, or a requester integration,
  run `/Users/joey/.local/bin/nucleus manual`. If it is unavailable, read
  `nucleus/docs/operator-manual.md`.
- Update `nucleus/docs/operator-manual.md` in the same change when shared
  operational facts, boundaries, or procedures change.
- Preserve product-scoped instructions in nested `AGENTS.md` files.
- Every code change must leave `./ci.sh` green. Each product CI has its own
  60-second deadline; the root dispatcher has no aggregate 60-second deadline.
