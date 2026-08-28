# Todo documentation

Todo converts an originating file and a directional need into one researched
todo, then supports a small deterministic lifecycle and an optional email
projection of the current open set.

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

       open todos -- preview / send -- Resend
                         ^
                         `-- launchd at 09:00 local time on macOS
```

The implemented contracts are:

- [CLI](cli.md): commands, selectors, email configuration, and output behavior;
- [Research liaison](liaison.md): prompt, permissions, and creation boundary;
- [Architecture](architecture.md): runtime components and failure semantics;
- [Data model](data-model.md): the two relational SQLite tables;
- [macOS installation](system-installation.md): user-owned deployment, the
  daily email LaunchAgent, and rollback.
