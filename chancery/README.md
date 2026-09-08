# Chancery

Chancery lists installed capabilities and reads their versioned operating
contracts. It can resolve one entry's complete promise, dependencies, and gaps.
It reads documentation; it does not execute the described operations or check
live readiness.

## Example

```sh
chancery list
chancery show ENTRY_ID
chancery resolve ENTRY_ID
```

Use the catalog to find plausible entries. Read their contracts before choosing
an interface.

## Check

From the Cell root:

```sh
./ci.sh chancery
```

## Further documentation

- [Commands and output](docs/cli.md)
- [Provider bundle format](docs/manifest.md)
- [Publish a provider](provider/manuals/provider-publish.md)
- [Architecture and Rust interface](docs/architecture.md)
- [Installation](docs/system-installation.md)
