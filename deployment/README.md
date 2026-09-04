# Selector-only deployment generation

`generate.py` renders complete, self-contained macOS deployers for products
whose deployment changes only an immutable program release and its command and
Chancery provider selectors. Product runtime state is outside this profile.
Generation requires Python 3.10 or newer; generated deployers themselves use
only the documented macOS shell tools.

Regenerate or check the checked-in scripts with:

```sh
python3 deployment/generate.py
python3 deployment/generate.py --check
```

The profile has no arbitrary shell hooks. A product that needs service control,
database work, maintenance, scheduling, authentication, or a domain canary keeps
a product-owned deployer. Every product that publishes a Chancery provider uses
the same catalog-writer lock; custom/stateful profiles also keep their existing
product and lifecycle locks and are conservatively declared as globally
conflicting to an orchestrator. The generated deployer stages outside the
shared catalog lock, takes its product lock before the catalog writer lock,
publishes one atomic `current` selector, and packages its exact own bytes for
rollback.

Each descriptor supplies only product identity/display names, application
support/binary/provider names, help text, the existing manifest-format toggle,
and existing completion-output variants. There are no lifecycle hooks.

`--expected-current absent|releases/<sha256>` supplies an optional optimistic
concurrency precondition. When omitted, the deployer snapshots `current` before
waiting for its product lock and rejects the operation if that selection changes.
