# Telete

Telete is an independent Rust CI system for Cell. It owns a serial delivery
queue, candidate validation, gate admission, repair, production preparation,
deployment, and outcome notification.

Build and inspect its command interface:

```sh
cargo run -p telete -- --help
```

Read the [operating contract](chancery/manuals/queue-operate.md) for state,
commands, prerequisites, failure handling, and provider boundaries. Read the
[product overview](chancery/overview.md) for the system structure.
