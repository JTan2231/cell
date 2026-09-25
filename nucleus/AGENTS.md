# Agent instructions

Semantics-Project: nucleus

- Keep changes simple.
- The installed Semantics service maintains this project's terminology and
  history. Before analysis or changes, read `semantics.repository.explore`
  through Chancery and query this project's repository. Code, tests, and product
  documentation define behavior. Do not edit Semantics state directly. If the
  repository cannot be resolved, report the problem.
- Before changing the public contract, harness compatibility, persistent state,
  authentication or service lifecycle, deployment, or a requester integration,
  read `docs/operator-manual.md`.
- Update `docs/operator-manual.md` in the same change when any of those
  operational facts, boundaries, or procedures change.
- Keep detailed feature behavior in the product's Chancery feature contracts.
  Keep operation manuals focused on procedures and action-critical conditions.
  Update the owning feature and affected procedures together. Use Chancery
  identities for installed documentation references; do not add another full
  explanation to the repository navigation pages.
