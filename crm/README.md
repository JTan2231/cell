# CRM

CRM is a private library for employment-related cases. It stores case history
and reusable career material as Markdown. An AI steward uses supplied updates
to revise a case. The caller handles messages and other contact actions.

## Example

With an initialized library:

```sh
crm case new --title "Example opportunity"
crm case list
crm tell CASE_ID update.md
crm update list
```

`tell` stores the update before it starts the worker. It returns before the AI
work finishes. Profile entries can be created and edited without AI.

## Check

From the Cell root:

```sh
./ci.sh crm
```

## Further documentation

- [Commands and recovery](docs/cli.md)
- [Installation and migration](docs/system-installation.md)
- [Architecture](docs/architecture.md) and [stored records](docs/data-model.md)
- [Rust interface](docs/rust-api.md)
- [Operating contracts](chancery/provider.json)
