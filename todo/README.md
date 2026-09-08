# Todo

Todo records concerns and the sources they came from. It helps research the
current situation, propose a design, and record explicit decisions. Research
produces proposals; acceptance and implementation are separate actions.

## Example

With an initialized library:

```sh
todo concern add "Need to report token consumption" --source /absolute/source.md
todo concern show c1
todo list
```

Use the concern ID returned by `concern add`. Assessing a concern uses Nucleus
and records a pending routing proposal.

## Check

From the Cell root:

```sh
./ci.sh todo
```

## Further documentation

- [Commands and records](docs/cli.md)
- [Installation and recovery](docs/system-installation.md)
- [Research stages](docs/liaison.md)
- [Architecture](docs/architecture.md) and [stored records](docs/data-model.md)
- [Rust interface](docs/rust-api.md)
- [Operating contracts](chancery/provider.json)
