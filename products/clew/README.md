# Clew

Clew is a private ledger notepad for supplied notes and status. Entries can
belong to named threads and carry optional links to external stable IDs.
It preserves append-only history and corrections. Explicit Milieu opportunity reports
also support application tracking and a daily application email.

```sh
clew record --id sla-start --thread 'SLA implementation' --notes 'Started the implementation.'
clew record --id sla-done --thread 'SLA implementation' --status done --notes 'Finished the implementation.'
clew search SLA
clew thread 'SLA implementation'
```

- [Product overview and feature inventory](chancery/overview.md)
- [Record and read a ledger notepad](chancery/manuals/ledger-use.md)
- [Record and read application history](chancery/manuals/application-track.md)
- [Preview and send the daily email](chancery/manuals/digest-email.md)
- [Private state and installation lifecycle](chancery/manuals/state.md)
- [Install Clew](chancery/manuals/install-operate.md)

Read the release publication with `chancery product clew`. Use
`chancery show ID` for a focused contract and `chancery resolve ID` for its
required contracts and compatibility gaps.
