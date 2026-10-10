# Paperboy

Paperboy runs configured productions and submits their output as plain-text
email through Email. Each production definition selects a renderer command,
optional subject, and Clockwork schedule. Scripts collect and render the data;
successful nonempty stdout becomes the exact email body.

## Example

Create an empty definition file and inspect production definitions:

```sh
paperboy init
paperboy list
paperboy doctor
paperboy schedule status
```

Adapt [the example production definitions](example.toml), then run `paperboy apply`
to register their schedules. New schedules remain disabled. Start one production
run with `paperboy run PRODUCTION_ID`; it can send real email. Enable future
scheduled runs with `paperboy schedule enable PRODUCTION_ID`.

## Further documentation

- [Production definitions, rendering, email, and limits](chancery/manuals/report-send.md)
- [Installation and schedule control](chancery/manuals/install-operate.md)
- [CI submission](../../infrastructure/telete/README.md)
