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

## Check

From the Cell root:

```sh
./ci.sh conversations
```

## Further documentation

- [Commands and output](docs/cli.md)
- [Architecture and Rust interface](docs/architecture.md)
- [Installation](docs/system-installation.md)
- [Operating contracts](chancery/provider.json)
