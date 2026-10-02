# Conversations

Conversations reads local Codex task history through App Server. It lists,
searches, and exports user and assistant messages from active and archived root
tasks. Optional flags include subagents and non-interactive tasks.

## Example

```sh
conversations list
conversations search 'approved design'
conversations show THREAD_ID
```

Message output and exported files can contain private conversation text.

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Product overview and contract inventory](chancery/overview.md)
- [History, metadata, and completed-turn activity](chancery/manuals/history-explore.md)
- [Runtime and installation guarantees](chancery/manuals/runtime.md)
- [Installation and recovery procedure](chancery/manuals/installation-operate.md)
- [Development procedure](chancery/manuals/develop-change.md)
