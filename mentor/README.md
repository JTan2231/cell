# Mentor

Mentor emails one authored system design problem each day. Reply with a complete
answer to receive an independent qualitative critique. Mentor keeps no answer
or critique archive.

## Example

Inspect an initialized installation and its schedule:

```sh
mentor --json status
mentor schedule status
```

The default delivery time is 09:00 in `America/Chicago`. Initialization leaves
Mentor paused. The installation guide describes how to enable delivery.

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Practice, records, and retention](chancery/manuals/practice-use.md)
- [Installation, schedule, and recovery](chancery/manuals/installation-operate.md)
- [Problem corpus](content/README.md)
- [Operating contracts](chancery/provider.json)
