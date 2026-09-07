# Krisis

Krisis observes eligible completed root user turns. It assigns each user
authority a `decision` or `no_decision` verdict and delivers deterministic
decision accounts to a dedicated Annals library. After Annals accepts an
account, Krisis keeps only the coverage, source anchors, digests, correlations,
and receipts needed for recovery and audit. Annals stores the accepted accounts.

The public executable and Chancery provider are `krisis`. The repository folder,
Rust package, database path, and log path retain the `decisions`/`Decisions` name
for migration compatibility with existing persistent history.

Start with [docs/README.md](docs/README.md). Development is gated by `./ci.sh`.
Release, deployment, live migration, hook trust, and live Annals acceptance are
separate operations and are not performed by CI.
