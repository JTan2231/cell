# Cell CI manager

The CI manager owns one durable, serial queue for committed Cell changes. It
merges each submitted commit into `refs/ci/accepted`, validates a private
candidate, obtains bounded read-only Nucleus repair proposals, applies accepted
patches, and requests deployment of the exact passing commit. Development
continues on `main`.

Submit a committed candidate and inspect its retained job:

```sh
cell-ci submit COMMIT
cell-ci status JOB
```

Read [queue operation](chancery/manuals/queue-operate.md) before initialization,
installation, submission, cancellation, or recovery. It defines submission
authority, model limits, Git patch application, deployment and email outcomes, and the
conditions that pause the queue.

Read [CI selection](../pipeline/README.md) for committed-range validation and
[the broker](../ci_broker/README.md) for individual gate execution. Read
[deployment](../deployment/README.md) for exact-source installation and recovery.
The manager is shared infrastructure; it has no product gate or automatic
self-deployment target.
