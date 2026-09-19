# Bazaar

Bazaar stores opaque strings under stable IDs with append-only versions.
Programs use its in-process Rust API. The CLI uses the same store.

```sh
bazaar init
bazaar update example.prompt --file prompt.txt
bazaar get example.prompt
bazaar get example.prompt --version 1
bazaar history example.prompt
```

See the [Rust API](docs/rust-api.md), [read contract](chancery/manuals/string-read.md),
[update contract](chancery/manuals/string-update.md), and
[installation guide](chancery/manuals/install-operate.md).
