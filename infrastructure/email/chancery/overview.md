# Email

Email is a synchronous current-user mail transport. It submits one caller's
plain-text message and optional attachments from `Codex <codex@joeytan.dev>` to
`j.tan2231@gmail.com`. It also reads one received-mail page or one received
message from the configured Resend account. Outgoing reply routing and thread
headers do not change the fixed sender or recipient.

Email owns request construction, bounded transport retries, decoded responses,
private local account settings, program selection, and command reporting.
The caller owns send/read authority, occurrence identity, scheduling, message
meaning, routing, deduplication, and retained workflow state. Resend owns account
records and submission acceptance; Gmail owns final delivery and classification.

Read the installed publication with `chancery product email`. Read one focused
feature or procedure with `chancery show ID`. Read an operation and its required
feature contracts with `chancery resolve ID`. These commands read documentation;
they do not load Email credentials, probe external providers, or access mail.

## Features

| ID | Read this to understand |
| --- | --- |
| `email.message.send` | Fixed-address submission, local or byte attachments, reply fields, idempotency, acceptance, bounded retries, and ambiguous failures. |
| `email.message.receive` | One-page account metadata reads, selected content, provider identities and timestamps, untrusted input, limits, and caller retention. |
| `email.account` | Private credential selection, atomic local settings, receiving-domain discovery, domain observations, and account-access limits. |
| `email.installation` | Retained program archives, fixed runtime paths, wrapper boundaries, matching documentation, selector recovery, and retained settings separation. |

## Operations

Use `email.account.operate` to select a supplied credential or receiving domain,
or discover domains without reading mail. Its required `email.account` contract
owns detailed setting and observation semantics.

Use `email.install.operate` for Cell delivery through CI and Telete or explicitly
authorized retained-release recovery. Account setup and any real-send
check retain their separate authority requirements.

## How the features work together

An installed wrapper preserves stdin and provides a scrubbed credential
fallback. Email selects an explicitly configured private credential first.
Send freezes one exact payload and key for bounded transport attempts. Receiving
returns one provider observation without changing account mail. Neither route
creates a queue, scheduler, daemon, or local mail history.

Account settings retain only selected credentials and domain configuration.
Program installation publishes programs at fixed runtime paths and selects
matching documentation from a retained archive. Program recovery and private settings
selection have separate boundaries. An installed program or selected domain
does not establish live account readiness.

Required dependencies declare compatible documentation and complete `resolve`
reading. Related entry references provide navigation. Neither form transfers
authority or executes a command. Explicit unspecified guarantees and the
uncontracted Resend reliance remain visible in resolution.
