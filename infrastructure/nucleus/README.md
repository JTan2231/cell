# Nucleus

Nucleus runs local agent jobs for the current user. It manages authentication,
executes up to eight jobs at a time, and retains job output. Each requesting
product owns its data and decides whether its work succeeded.

## Example

With an installed, authenticated service:

```sh
nucleus health
nucleus jobs submit /Users/joey/rust/cell/infrastructure/nucleus/examples/job.example.json
nucleus jobs show explain-unix-socket-01
nucleus jobs logs --follow explain-unix-socket-01
```

The example submits a real agent job. `health` returns a nonzero exit status
if the service is incompatible, unauthenticated, or closed to new jobs.

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

Read the installed Nucleus overview and feature inventory through Chancery:

```sh
chancery product nucleus
chancery show nucleus.jobs
chancery resolve nucleus.execution.operate
```

Use `nucleus.requester.integrate` for integration and `nucleus.develop.change`
for development. The [shared operator manual](docs/operator-manual.md), also
available as `nucleus manual`, covers ecosystem coordination and recovery order.
