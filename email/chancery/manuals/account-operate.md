# Configure Email account access

Email owns local credential setup and receiving-domain discovery. It does not
create remote keys, change account permissions or DNS, or read mail during setup.

```sh
email setup --credential-file /absolute/private/key --receiving-domain account.resend.app
email receive settings
```

Both setup arguments are optional. Omitted values retain their current selection.
The source credential must be an absolute regular file, without symbolic or hard
links or group/other permissions. It contains at most 4096 bytes and one nonblank
credential without internal whitespace. Its contents never appear in a receipt.

Email atomically writes each supplied setting beneath
`~/Library/Application Support/Email/settings`. The directory is private and files
use mode 0600. `resend-api-key` takes precedence over the wrapper's existing
`RESEND_API_KEY` fallback. `receiving.json` retains the domain selection. If a
later setting fails, an earlier selection remains; retry the same inputs. The
source credential stays caller-owned. Never copy its contents into deployment
JSON, agent context or schedule definitions.

Cell deployment accepts `credential_file` and `receiving_domain` in Email's
settings object. Inspection validates inputs before maintenance; configuration
performs the same local setup. It returns only whether fields were supplied.

`receive settings` returns `{"domains":["account.resend.app"]}` for an explicit
selection. That selection is operator configuration, not remote verification.
Without local selection, Email traverses Resend's domain list and returns names
with enabled receiving and a verified `Receiving MX` record. It never uses
received messages to infer an account domain. Managed `*.resend.app` domains
require explicit selection because the provider does not document a supported
discovery endpoint for them. Empty or multiple results require caller selection.

Domain discovery reads at most 1000 domains in pages of at most 100. Repeated
IDs, incomplete pages, missing access or malformed data fail without partial
output. Each request uses the receiving transport's 30-second timeout, at most
two transient retries and 8 MiB bound. The installed Rust
`email::api::Client::receiving_settings` bounds the entire command to 120 seconds
and 64 KiB output. Domain observations do not promise future receiving readiness.
No message metadata, content, credential or provider error body is returned.

Provider references: [domain list](https://resend.com/docs/api-reference/domains/list-domains),
[domain detail](https://resend.com/docs/api-reference/domains/get-domain),
[receiving MX status](https://resend.com/docs/webhooks/domains/updated).

## Command usage

CLI dispatch separately attempts to append system/command identity, observation
time and optional `CODEX_THREAD_ID` to Chancery's private usage journal. It
records invocation only, retains no arguments or output, and preserves product
results after recording errors. `--register-usage` is the separate post-install
step that adds the program's complete command inventory without product work.
