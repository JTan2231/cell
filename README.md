# Annals

Annals is a proposed local-first CLI for maintaining and searching trees of
textual topics. Each child is a more detailed view of its parent; completed
trees end in source material.

This repository currently contains the base design only. Start with the
[documentation index](docs/README.md).

The initial design deliberately uses:

- Rust for the CLI;
- SQLite as the authoritative store;
- SQLite FTS5 for search;
- no embeddings, generated topics, or external services.
