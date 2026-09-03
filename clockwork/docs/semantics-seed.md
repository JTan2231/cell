# Clockwork semantic seed

This project-local definition list describes only Clockwork v0.1 behavior. It
is input prepared for a later explicit `semantics repository seed-markdown`
operation; its presence does not mean the `clockwork` Semantics project has
been registered or seeded.

**Scheduled activation**
: One admitted invocation of one immutable Clockwork definition, resulting in at most one directly supervised child process and one runtime-history record. It is not a product-domain unit of success.

**Definition**
: An immutable normalized Clockwork snapshot containing one stable key's schedule, product-supplied exact release identity, pinned top-level launch image, literal process context, output paths, authority, timeout, and overlap policy; its Clockwork identity is the SHA-256 digest of its canonical concrete content.

**Binding**
: The stable `owner/name` identity whose current nullable selection points to one immutable definition and whose generated LaunchAgent contains only that stable key.

**Executable**
: The registered top-level launch image Clockwork verifies and directly spawns. The term does not include transitive libraries, later-opened configuration, subprocesses, same-user tamper resistance, or product meaning.

**Direct launch**
: A launch whose exact absolute executable program path, recognized Mach-O/fat magic, and SHA-256 are registered and invoked without PATH lookup, a shell, interpolation, or an implicit interpreter; admission is not a complete host-loader preflight.

**Interpreted launch**
: A schema-one launch whose exact root-owned `/bin/sh` and release-local executable script paths and separate SHA-256 values are registered, with `/bin/sh` directly receiving the script as its first operand rather than using a command string or implicit shebang.

**Activation key**
: A stable lowercase `owner/name` binding identity supplied at runtime instead of caller-controlled executable, arguments, environment, working directory, schedule, or policy.

**Runtime success**
: Evidence that Clockwork admitted, started, and observed the direct child according to the recorded state. It never means the owning product completed its domain work.

**Domain success**
: The owning product's interpretation of its durable work and outcome, outside Clockwork state and authority.

**Skipped overlap**
: A terminal activation record stating that another activation held the stable key's admission lock and no new child was started.

**Binding cutover**
: The guarded transition that aligns the selected definition, generated plist bytes, and launchd loaded state. It restores the prior coherent state or, while the projection remains attributable to Clockwork, durably attempts a disabled state. An unattributable projection is retained, reported, and recovery-gated without destructive mutation.

**Clockwork frontend**
: The exact installed content-addressed Clockwork binary path embedded in generated plists and used only to resolve a stable activation key through Clockwork-owned state.

**Product output**
: The direct child's stdout or stderr bytes appended to distinct exact product-selected private files. Clockwork validates and opens those destinations but never ingests their bodies or owns their meaning or retention.
