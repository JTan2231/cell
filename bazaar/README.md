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

Read the [product overview](chancery/overview.md) for the feature contracts and
operating procedure. The [Rust API entry point](docs/rust-api.md) directs
programs to those same contracts.

Installed documentation is available through `chancery product bazaar`,
`chancery show ID`, and `chancery resolve ID`.
