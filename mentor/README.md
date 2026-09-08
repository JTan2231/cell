# Mentor

Mentor sends one authored system design problem each day. Reply with a complete
answer to receive a qualitative critique. The problem email contains the
problem only; the title is its subject. Each answer is assessed independently.
There is no score history, answer archive, or ongoing tutor conversation.

This Cell product uses a bundled export from the desktop Mentor authoring app
at `/Users/joey/ts/mentor`. The desktop app does not need to run. The initial
bundle contains 58 problems and their shared rubric. See the
[corpus contract](content/README.md) for identity and refresh rules.

Email owns sending, reply headers, receiving reads, and the Resend credential.
Nucleus owns constrained model execution. Clockwork activates Mentor's worker.
Mentor owns daily selection, assignment routing, short-lived work, and accepted
send receipts. Email fixes the personal recipient; incoming addresses cannot
change that destination.

The default daily time is 09:00 in `America/Chicago`. Initialization leaves
Mentor paused. Installation, initialization, resuming admission, and enabling
the schedule are separate operations. Source files alone enable none of them.

```sh
mentor --json status
mentor configure --time 09:00 --timezone America/Chicago
mentor pause
mentor resume
mentor schedule status
```

The private database is
`~/Library/Application Support/MentorMail/mentor.sqlite3`. This location is
separate from the desktop app's `Mentor` state. Pending answer/request/critique
text expires after 24 hours and is cleared when a worker pass observes expiry;
an accepted critique submission clears it immediately. Assignment and corpus
records remain for no-repeat selection and late answers. Ordinary terminal
mail metadata expires after 35 days. Nucleus, Resend, and the inbox provider
retain their records separately. This is not a zero-retention system.

- [Service behavior, records, and commands](docs/service.md)
- [Installation, schedule, and maintenance](docs/system-installation.md)
- [Chancery source bundle](chancery/provider.json)

The source bundle contains `mentor.practice.use`,
`mentor.installation.operate`, and `mentor.development.change`. It becomes
installed discovery only when staged and selected with a Mentor release.
