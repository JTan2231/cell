# Agent instructions

Semantics-Project: cast

- Keep changes and architecture simple.
- Query this product's registered Semantics repository through its installed
  contract before analysis or changes. Code, tests and product documentation
  remain authoritative for behavior. Do not edit Semantics state directly.
- Cast owns stored companies and jobs, source metadata, collection state,
  local budgets and the read handoff. Selection, application packets, CRM cases
  and email delivery belong to downstream products.
- Collection uses code and HTTP. Do not add a fallback that uses a model,
  computer use, Nucleus or the CRM steward.
- Keep source contracts and the Chancery bundle aligned with actual behavior.
  Update the shared Nucleus operator manual when operational boundaries change.
- Every code change must leave the root `./ci.sh` green.
