# Retained Python CI manager

The Python manager retains Cell's former serial delivery queue, job history,
and `refs/ci/accepted` source history. Use `cell-ci` for its records and supported
recovery. Keep its worker stopped while [Telete](../infrastructure/telete/README.md)
owns new delivery work. Root and product `ci.sh` wrappers select Telete.

Inspect the retained queue or one job:

```sh
cell-ci status
cell-ci status JOB
```

Read [queue operation](chancery/manuals/queue-operate.md) before Python manager
maintenance, cancellation, or recovery. It defines frozen job policies, model
limits, deployment and email outcomes, and unresolved-effect recovery. Telete
does not import this journal, private refs, or provider receipts.

Read [external work storage](STORAGE.md) for the shared volume, retained Python
setup commands, and drive-loss recovery. Read
[macOS signing](chancery/manuals/signing-operate.md) for Cell's shared host
certificate and private-key recovery.

Read [retained validation](../pipeline/README.md#retained-python-validation-and-manual-release-support),
[the Python broker](../ci_broker/README.md), and
[the Python deployment helpers](../deployment/README.md) for their separate
interfaces and retained evidence. The Python manager has no product gate or
automatic self-deployment target.
