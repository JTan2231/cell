# Telete

Telete is Cell's Rust CI system. It owns a serial delivery
queue, candidate validation, gate admission, repair, production preparation,
deployment, and outcome notification.

Submit a committed change through the installed program:

```sh
telete submit COMMIT --repo /absolute/cell
telete status JOB
```

Read the [operating contract](chancery/manuals/queue-operate.md) for state,
commands, prerequisites, failure handling, and provider boundaries. Read the
[product overview](chancery/overview.md) for the system structure.
