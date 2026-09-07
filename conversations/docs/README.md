# Conversations documentation

- [Architecture](architecture.md)
- [CLI](cli.md)
- [System installation](system-installation.md)

The product-owned [Chancery provider](../chancery/provider.json) indexes the
history-exploration and development contracts. Find the relevant entry with
`chancery list` and `chancery show`; after selecting an exact entry, run
`chancery resolve <ENTRY_ID>` to read its provider scope, contract, dependency
contracts, sources, and declared gaps. Resolution does not check App Server,
authorize an operation, or retrieve history.
