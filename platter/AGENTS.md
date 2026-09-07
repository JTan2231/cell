# Platter

Semantics-Project: cell

- Keep this requester simple. Cast owns discovery, CRM owns career material,
  Nucleus owns execution, and Platter owns preparation and delivery state.
- Until a separate semantic repository is registered, query Cell terminology.
- Preserve the captured original resume byte-for-byte outside its Jackson
  bullet span. Models supply plain-text Jackson bullets, never a replacement
  resume or LaTeX document. Never commit the user's private resume or profile.
- Keep accepted stages and exact send occurrences durable across failures.
- Every code change must leave the root `./ci.sh` green.
