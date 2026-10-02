# Email account access and receiving domains

Email owns local credential selection and receiving-domain discovery. Resend
owns key permissions, account records, and domain verification. The caller owns
setup authority and the choice among receiving domains. These interfaces do
not create remote keys, change account permissions or DNS, or read mail.

Read `chancery show email.account.operate` for setup and discovery steps.

## Interfaces and inputs

```sh
email setup --credential-file /absolute/private/key --receiving-domain account.resend.app
email receive settings
```

Both setup arguments are optional. Omitted values retain their current selection.
The credential source must be an absolute regular file, without symbolic or hard
links or group/other permissions. It contains at most 4096 bytes and one nonblank
credential without internal whitespace. Its contents never appear in a receipt.
DNS domain names are case-normalized. Malformed names fail selection.

## Stored settings and credential authority

Email atomically writes each supplied setting beneath
`~/Library/Application Support/Email/settings`. The directory is private and files
use mode 0600. `resend-api-key` takes precedence over `RESEND_API_KEY` loaded
by the installed wrapper or supplied by a direct caller. `receiving.json`
retains the domain selection. The source credential stays caller-owned.

Each file replacement is atomic and synced. Setup is not a two-file transaction.
If a later setting fails, an earlier selection remains. Repeating setup safely
applies the same supplied inputs. A successful setup returns
`{"configured":true}`. It does not prove remote access or receiving readiness.

Cell deployment accepts only `credential_file` and `receiving_domain` in Email's
settings object. Inspection validates inputs before maintenance; configuration
performs the same local setup. It returns only whether fields were supplied.
Omitted settings remain selected. Credentials never enter deployment JSON,
agent context, schedule definitions, receipts, or errors.

## Domain selection and observations

`receive settings` returns `{"domains":["account.resend.app"]}` for an explicit
selection. That selection is operator configuration, not remote verification.
Without local selection, Email traverses Resend's domain list and returns names
with enabled receiving and a verified `Receiving MX` record. It never uses
received messages to infer an account domain. Managed `*.resend.app` domains
require explicit selection because the provider does not document a supported
discovery endpoint for them. Empty or multiple results require caller selection.

`ReceivingSettings` contains a sorted, deduplicated array of DNS domain names.
These names describe routing, not authenticated senders, message recipients,
or a future delivery guarantee. Provider pages are successive observations,
not an atomic account snapshot. No message metadata or content is read.

## Limits and recovery

Domain discovery reads at most 1000 domains in pages of at most 100. Repeated
IDs, incomplete pages, missing access, and malformed data fail without partial
output. Each request has a 30-second timeout, at most two transient retries,
and an 8 MiB response bound.

`email::api::Client::receiving_settings` invokes the selected absolute installed
wrapper without reading its credential. It bounds the whole command to 120
seconds and 64 KiB output. Domain observations do not promise future receiving
readiness. Credentials and provider error bodies are omitted from output.
Domain API requests disclose account selection to Resend.

Unsafe credential sources or owned settings paths stop setup. Keep an earlier
successful setting when a later field fails, then repeat the authorized setup
with the same supplied inputs. Never infer that a remote key or DNS change
occurred. Setup and domain-read success do not grant mail-read or send authority.

Provider references: [domain list](https://resend.com/docs/api-reference/domains/list-domains),
[domain detail](https://resend.com/docs/api-reference/domains/get-domain),
[receiving MX status](https://resend.com/docs/webhooks/domains/updated).
Resend has no dedicated installed contract. Its availability and account-domain
observations remain an explicit resolver gap.

## Related contracts

- Read `chancery show email.account.operate` for setup and discovery steps.
- Read `chancery show email.installation` for program and wrapper ownership.
- Read `chancery show email.message.send` for send authority and disclosure.
- Read `chancery show email.message.receive` for account-mail read authority.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.
