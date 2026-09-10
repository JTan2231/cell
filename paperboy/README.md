# Paperboy

Paperboy emails reports of local Codex conversations or Krisis decisions accepted
into Annals. A Nucleus agent reads the selected source and writes the report.
Email submits it to the fixed personal recipient.

## Example

Read retained reports and inspect the schedule:

```sh
paperboy list
paperboy show BRIEF_ID
paperboy preview BRIEF_ID
paperboy schedule status
```

`paperboy run --ad-hoc` creates a new report and sends a real email. The daily
schedule starts at local 09:00 when the required macOS session is available.

## Check

From the Cell root:

```sh
./ci.sh paperboy
```

## Further documentation

- [Reports, timeframes, privacy, and recovery](chancery/manuals/report-send.md)
- [Installation and scheduling](chancery/manuals/install-operate.md)
