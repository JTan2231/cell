# Agent instructions

Semantics-Project: milieu

- Keep changes and architecture simple.
- Query this product's registered Semantics repository through its installed
  contract before analysis or changes. Code, tests and product documentation
  remain authoritative for behavior. Do not edit Semantics state directly.
- Milieu owns accepted current companies and opportunities, source metadata, workplaces,
  source appearances and the read handoff. Selection, application packets, CRM
  cases and email delivery belong to downstream products.
- Milieu performs no collection or runtime HTTP requests. Do not add a fallback
  that uses a model, computer use, Nucleus or the CRM steward.
- Keep source contracts and the Chancery bundle aligned with actual behavior.
  Update the shared Nucleus operator manual when operational boundaries change.
- Keep complete feature explanations in the owned Chancery contracts. Keep
  prerequisites, consequential effects, stop conditions, and verification in
  operation manuals. Update the owning feature and affected procedures together.
  Use the overview, README, and terminology pages for navigation; do not keep
  competing behavior explanations there.
- Submit committed code changes through `./ci.sh submit COMMIT` from the Cell
  root and verify the manager job outcome. Follow the root CI instructions.
