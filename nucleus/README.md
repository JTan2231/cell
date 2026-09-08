# Nucleus

Nucleus runs local agent jobs for the current user. It manages authentication,
executes up to eight jobs at a time, and retains job output. Each requesting
product owns its data and decides whether its work succeeded.

## Example

With an installed, authenticated service:

```sh
nucleus health
nucleus jobs submit /Users/joey/rust/cell/nucleus/examples/job.example.json
nucleus jobs show explain-unix-socket-01
nucleus jobs logs --follow explain-unix-socket-01
```

The example submits a real agent job. `health` returns a nonzero exit status
if the service is incompatible, unauthenticated, or closed to new jobs.

## Check

From the Cell root:

```sh
./ci.sh nucleus
```

## Further documentation

- [Shared operator manual](docs/operator-manual.md), also available as `nucleus manual`
- [Installation and recovery](docs/system-installation.md)
- [Runtime contract](docs/runtime-contract.md)
- [Requester integration](chancery/manuals/requester-integrate.md)
- [Operating contracts](chancery/provider.json)
