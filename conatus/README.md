# Conatus

Conatus preserves wants in their source wording. It uses a dedicated Annals
library to organize accepted decision accounts by which wants they appear to
serve. These associations describe an interpretation; they do not measure
progress or completion.

## Example

Inspect an initialized library:

```sh
conatus want list --limit 20
conatus decision list --limit 20
conatus status
```

## Check

From the Cell root:

```sh
./ci.sh conatus
```

## Further documentation

- [Commands and recovery](docs/cli.md)
- [Records and interpretation](docs/design.md)
- [Installation and activation](docs/installation.md)
- [Operating contracts](chancery/provider.json)
