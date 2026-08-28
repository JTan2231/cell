# Todo documentation

Todo converts an originating file and a directional need into one researched
todo, then supports a small deterministic lifecycle.

```text
source path + direction
          |
    read-only research
          |
          v
 managed create_todo call
          |
          v
       open todo -- done / reopen
          |
          `-- append-only working notes
```

The implemented contracts are:

- [CLI](cli.md): commands, selectors, and output behavior;
- [Research liaison](liaison.md): prompt, permissions, and creation boundary;
- [Architecture](architecture.md): runtime components and failure semantics;
- [Data model](data-model.md): the two relational SQLite tables;
- [macOS installation](system-installation.md): user-owned deployment and
  rollback.
