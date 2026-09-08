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

## Check

From the Cell root:

```sh
./ci.sh email
```

## Further documentation

- [Sending commands and Rust interface](docs/cli.md)
- [Read received mail](chancery/manuals/message-receive.md)
- [Installation and credentials](docs/system-installation.md)
- [Send effects and recovery](chancery/manuals/message-send.md)
