# Clockwork

Clockwork starts scheduled programs for Cell products. A product registers a
fixed program definition and selects it with an `owner/name` binding.
Clockwork supervises each process and records its outcome. The product owns
its work, retries, and interpretation of success.

## Example

Inspect the configured bindings and recent activations:

```sh
clockwork binding list
clockwork history --limit 20
clockwork doctor
```

## Check

From the Cell root:

```sh
./ci.sh clockwork
```

## Further documentation

- [Commands and definition format](docs/cli.md)
- [Installation and recovery](docs/system-installation.md)
- [Architecture and Rust interface](docs/architecture.md)
- [Stored records](docs/data-model.md)
- [Operating contracts](chancery/provider.json)
