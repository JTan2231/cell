# Usher

Usher checks how products declare their membership in Cell. It reads product
identity, Semantics participation, and Chancery introductions from repository
files. These declarations do not establish installation or runtime readiness.

## Example

From a Cell checkout with a built Usher binary:

```sh
target/release/usher report .
target/release/usher check .
```

`report` shows the evidence. `check` reports incomplete selected products and
returns a nonzero exit status when declarations are incomplete or invalid.

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [Telete](../../infrastructure/telete/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Product overview and feature inventory](chancery/overview.md)
- [Recognition rules, output, and limits](chancery/manuals/recognition-inspect.md)
- [Installation guarantees](chancery/manuals/installation.md)
- [Installation and recovery](chancery/manuals/install-operate.md)
- [Development](chancery/manuals/develop-change.md)

Read the installed overview with `chancery product usher`, one feature or
procedure with `chancery show ID`, and its required contracts with
`chancery resolve ID`.
