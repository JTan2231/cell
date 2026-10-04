# Telete

Telete implements Cell CI orchestration in Rust. Root and product `ci.sh`
wrappers select installed Telete. Telete owns the executable, journal, worktrees,
Git references, deployment records, and user service.

Explicit host setup writes Cell's shared external-volume and signing selections.
It preserves the selected volume and signing identity. Telete maintenance guards
coordinate signing changes with queue work, deployment, and release publication.
Queue operation remains separate from host setup.

The manager owns submission and source acceptance. The candidate validator owns
gate selection. The broker owns gate admission and process results. Production
preparation owns compilation, staging, and configured native signing. The
deployment executor performs product-owned instructions in order.

Nucleus owns agent execution. Bazaar owns versioned prompt text. Email owns
message transport. Chancery owns provider document formats. Usher owns Cell
membership recognition. Telete uses their supported Rust interfaces and retains
its own domain decisions.

Read `telete.queue.operate` for delivery commands, retained state, and recovery.
Read `telete.signing.operate` for shared host storage, signing identity, and
maintenance. Telete has no public Rust client, Iatreion probe, or checked evidence
type protocol. Its command interface uses ordinary records and runtime checks.
