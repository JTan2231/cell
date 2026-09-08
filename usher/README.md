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

## Check

```sh
./ci.sh usher
```

## Further documentation

- [Recognition rules, output, and limits](chancery/manuals/recognition-inspect.md)
- [Installation and recovery](chancery/manuals/install-operate.md)
- [Development](chancery/manuals/develop-change.md)
