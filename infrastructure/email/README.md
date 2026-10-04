# Email

Email sends plain-text messages and attachments through Resend from
`Codex <codex@joeytan.dev>` to `j.tan2231@gmail.com`. It also reads received mail
from the configured Resend account.

## Example

With the installed credential configured, this command sends immediately:

```sh
email 'Subject' - < body.txt
```

A successful command means Resend accepted the submission. It does not confirm
arrival in the recipient's inbox.

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [Telete](../../infrastructure/telete/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

Read `chancery product email` for the installed overview and feature inventory.
Read `chancery show ID` for one feature or procedure, and `chancery resolve ID`
for required contracts.

- [Product overview and feature inventory](chancery/overview.md)
- [Send messages and attachments](chancery/manuals/message-send.md)
- [Read received account mail](chancery/manuals/message-receive.md)
- [Account access and receiving domains](chancery/manuals/account.md)
- [Installation guarantees](chancery/manuals/installation.md)
- [Configure account access](chancery/manuals/account-operate.md)
- [Install or recover Email](chancery/manuals/install-operate.md)
