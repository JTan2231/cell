# Iatreion agent instructions

Semantics-Project: iatreion

- Keep Iatreion stateless, local, deterministic, and read-only.
- Until the `iatreion` Semantics project is registered, use the Cell semantic
  repository for shared terminology. Code, tests, and product documentation
  define behavior. Do not edit Semantics state directly.
- Preserve product authority. Iatreion validates, joins, and presents
  observations. It does not decide product domain success.
- A failed, missing, incompatible, or stale observation is explicit unknown
  coverage. Never reuse an older successful result as current evidence.
- Do not add a daemon, database, network check, model call, alert, repair, or
  automatic retry.
- Every code change must leave the root `./ci.sh` green.
