# Cell CI manager

The CI manager owns one durable, serial queue for committed Cell changes. It
merges each submitted commit into `refs/ci/accepted`, validates a private
candidate, obtains bounded read-only Nucleus repair proposals, applies accepted
patches, and requests deployment of the exact passing commit. Development
continues on `main`.

New jobs refund a repair budget point when Git accepts the patch and the manager
records its private candidate commit. The default budget permits three
unrefunded Terra medium attempts, then one unrefunded Sol high attempt. A
refund does not depend on the next validation result. Successful patches can
therefore exceed four total invocations; failed attempts remain charged for
the whole job. Jobs submitted under the older policy keep their original limit.

Submit a committed candidate and inspect its retained job:

```sh
./ci.sh submit COMMIT
./ci.sh status JOB
```

Read [queue operation](chancery/manuals/queue-operate.md) before initialization,
installation, submission, cancellation, or recovery. It defines submission
authority, model limits, Git patch application, deployment and email outcomes, and the
conditions that pause the queue.

The root and product `ci.sh` wrappers use this manager. Bare invocations and
direct validation flags are unsupported. The manager invokes the internal
validator against its committed candidate and fixed accepted base.

Read [CI selection](../pipeline/README.md) for committed-range validation and
[the broker](../ci_broker/README.md) for individual gate execution. Read
[deployment](../deployment/README.md) for exact-source installation and recovery.
The manager is shared infrastructure; it has no product gate or automatic
self-deployment target.
