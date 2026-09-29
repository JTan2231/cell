# Configure Email account access

Use this operation to select a supplied credential or receiving domain, or to
discover receiving domains without reading mail. Read
`chancery resolve email.account.operate` for this procedure and the required
`email.account` feature contract.

## Preconditions and effects

Obtain authority for local setup or account-domain inspection. A supplied
credential must already exist in an absolute private regular file with no
symbolic or hard links and no group/other permissions. It must contain one
nonblank whitespace-clean credential and at most 4096 bytes. Discovery needs
an explicit local domain or domain-list/read access and network availability.

Setup retains supplied settings in Email's private mode-0600 files. Omitted
fields remain selected. Discovery reads local selection or Resend domain
records, then returns domain names. Neither operation reads or sends mail,
creates a remote key, changes key permissions, or changes DNS.

## Select local settings

1. Select the authorized private credential file and receiving domain. Keep
   credential bytes out of arguments, messages, agent context, and deployment JSON.
2. Run setup with the fields that need selection:

   ```sh
   email setup --credential-file /absolute/private/key --receiving-domain account.resend.app
   ```

3. Verify `{"configured":true}`. If a later field fails, keep the earlier
   selected setting and repeat the same supplied inputs. Each file is atomic;
   the two fields are not one transaction.

For managed Cell deployment, supply `credential_file` and `receiving_domain`
in Email's settings object. Supply a file path, never credential bytes.
Inspection validates inputs before maintenance; configuration uses the same
setup. Reuse the current credential when no new file is supplied.

## Read receiving-domain settings

1. Run the domain read:

   ```sh
   email receive settings
   ```

2. Inspect `domains`. An explicit local domain is operator selection; a
   discovered domain has enabled receiving and a verified receiving MX record.
   Neither result proves future receiving readiness.
3. Select exactly one domain in the requesting product before activation. Use
   explicit selection for managed `*.resend.app` domains. Stop when a required
   domain choice or account access is missing.

An empty or multiple-domain result is not an account change. Domain reads
return a complete selection or an error; retain no partial result. Resolve
the reported cause and repeat only the authorized read.

## Stop conditions and verification

Stop when the supplied file or owned settings path has unsafe ownership,
links, or permissions. Stop when discovery lacks required access, fails, or
does not establish the requesting product's selected domain. Preserve omitted
settings and any earlier atomic selection after a later failure.

Verify that receipts contain no credential bytes and that the caller records
the selected domain separately from remote readiness. Setup does not grant a
requester mail-read, recurring-send, or remote-account administration authority.

Read `chancery show email.account` for complete setting, observation, privacy,
and transport semantics. Read `chancery show email.install.operate` for program
installation and recovery.

## Command usage

CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.
