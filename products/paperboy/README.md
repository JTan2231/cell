# Paperboy

Paperboy runs configured scripts and sends their successful nonempty stdout as
plain-text email through Email. Scripts collect and render the data. A small
TOML manifest selects each command, optional subject, and Clockwork schedule.

## Example

Create an empty manifest and inspect configured jobs without executing them:

```sh
paperboy init
paperboy list
paperboy doctor
paperboy schedule status
```

Adapt [the example manifest](example.toml), then run `paperboy apply` to register
its schedules. New jobs remain disabled. `paperboy run JOB_ID` executes a script
and can send real email. Enable recurring execution with `paperboy schedule
enable JOB_ID`.

## Further documentation

- [Manifest, rendering, email, and limits](chancery/manuals/report-send.md)
- [Installation and schedule control](chancery/manuals/install-operate.md)
- [CI submission](../../infrastructure/telete/README.md)
