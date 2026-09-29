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

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Documentation

Read `chancery product chancery` for the installed overview and inventory.
Use `chancery show ID` for a focused feature or procedure and
`chancery resolve ID` for its required contracts.

- [Product overview](provider/overview.md)
- [Catalog, CLI output, and Rust client](provider/manuals/directory-discover.md)
- [Outward-promise resolution](provider/manuals/capability-resolve.md)
- [Provider bundle format and validation](provider/manuals/bundle-validate.md)
- [Command usage feature and API](provider/manuals/usage-record.md)
- [Provider publication](provider/manuals/provider-publish.md)
- [Reader installation and recovery](provider/manuals/installation-operate.md)
- [Usage registration, backup, and restore](provider/manuals/usage-operate.md)
