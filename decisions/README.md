# Krisis

Krisis observes eligible completed root user turns, assigns every user authority
a binary `decision` or `no_decision` verdict, and delivers each deterministic
decision account to a dedicated Annals library. Krisis is capture and transport,
not the decision library: after Annals accepts an account, Krisis keeps only the
minimal coverage, source-anchor, digest, correlation, and receipt ledger needed
for recovery and audit.

The public executable and Chancery provider are `krisis`. The repository folder,
Rust package, database path, and log path retain the `decisions`/`Decisions` name
for migration compatibility with existing persistent history.

Start with [docs/README.md](docs/README.md). Development is gated by `./ci.sh`.
Release, deployment, live migration, hook trust, and live Annals acceptance are
separate operations and are not performed by CI.
