# Iatreion

Iatreion reports the current operational state of the products declared by a
Cell checkout. It runs bounded, read-only product probes and keeps runtime and
domain outcomes distinct.

```sh
iatreion report /Users/joey/rust/cell
iatreion show annals/inbox --root /Users/joey/rust/cell --json
```

See [the status contract](docs/status-contract.md) and the
[installed capability manual](chancery/manuals/status-inspect.md).
