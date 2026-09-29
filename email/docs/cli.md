# CLI contract

Email's authoritative command and Rust interfaces are published with its release.
Read `chancery product email` for the product overview and inventory. Read
`chancery show email.message.send` for sending and
`chancery show email.message.receive` for received-account mail reads.

Repository readers can open the [sending feature](../chancery/manuals/message-send.md),
[receiving feature](../chancery/manuals/message-receive.md), or
[product overview](../chancery/overview.md).

## Byte payloads on stdin

Read `chancery show email.message.send` for `--payload-stdin`, its JSON input,
attachment ordering, input validation, and idempotent payload requirements.

## Reply routing and threads

Read `chancery show email.message.send` for reply routing, RFC Message-IDs,
thread header limits, and reserved CLI forms.

## Rust interface

Read `chancery show email.message.send` for direct sending functions and the
installed-wrapper client. Read `chancery show email.message.receive` for typed
receiving values and bounds. Read `chancery show email.account` for the
installed-wrapper receiving-settings interface and credential selection.
