# Agent instructions

Semantics-Project: cast

- Keep changes simple; do not overcomplicate or overarchitect.
- Query this product's registered Semantics repository through its installed
  contract before analysis or changes. Code, tests and product documentation
  remain authoritative for behavior. Do not edit Semantics state directly.
- Cast owns company and job discovery, evidence, freshness, collection state,
  local budgets and the read handoff. Selection, application packets, CRM cases
  and email delivery belong to downstream products.
- Ordinary collection uses code and HTTP. Do not add a model, computer-use,
  Nucleus or CRM-steward fallback.
- Keep source contracts and the Chancery bundle aligned with actual behavior.
  Update the shared Nucleus operator manual when operational boundaries change.
- Every code change must leave the root `./ci.sh` green.
