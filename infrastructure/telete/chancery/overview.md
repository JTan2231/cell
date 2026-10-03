# Telete

Telete implements Cell CI orchestration in Rust. It is separate from the
installed Python CI manager. Telete uses its own executable, journal, worktrees,
Git references, deployment records, and user service.

The manager owns submission and source acceptance. The candidate validator owns
gate selection. The broker owns gate admission and process results. Production
preparation owns compilation, staging, and configured native signing. The
deployment executor performs product-owned instructions in order.

Nucleus owns agent execution. Bazaar owns versioned prompt text. Email owns
message transport. Chancery owns provider document formats. Usher owns Cell
membership recognition. Telete uses their supported Rust interfaces and retains
its own domain decisions.

Read `telete.queue.operate` for commands, retained state, prerequisites, and
recovery. Telete has no public Rust client, Iatreion probe, or checked evidence
type protocol. Its command interface uses ordinary records and runtime checks.
