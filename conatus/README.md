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

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Product overview and feature inventory](chancery/overview.md)
- [Initialization, processing, recovery and installation procedures](chancery/manuals/update-operate.md)
- [Published provider bundle](chancery/provider.json)

Read `chancery product conatus` for the installed overview, `chancery show ID`
for one feature or procedure, and `chancery resolve ID` for required contracts
and compatibility gaps.
