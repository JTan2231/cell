# Clew

Clew records your application status and notes for jobs retained by Cast.
It keeps an append-only history and the latest status you supplied.
Its daily email shows tracked applications except current rejections, with
retained job details and saved notes.

```sh
clew find 'company or job URL'
clew record --cast-job JOB_ID --id my-application --status applied --notes 'Applied today.'
clew list
```

- [Record and read application history](chancery/manuals/application-track.md)
- [Preview and send the daily email](chancery/manuals/digest-email.md)
- [Install and verify Clew](chancery/manuals/install-operate.md)
