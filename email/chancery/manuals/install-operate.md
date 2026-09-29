# Install or recover Email

Use this operation for ordinary Cell delivery or explicitly authorized manual
installation and rollback. Read `chancery resolve email.install.operate` for
this procedure and the required `email.installation` feature contract. Read
`nucleus manual` before coordinated deployment or recovery.

## Preconditions and effects

Select a validated source candidate and matching trusted tested Email binary,
Rust installer, and provider bundle. Preserve private account settings and
existing caller authority. A program installation does not create a remote
account, key, verified sending domain, or receiving domain.

Installation publishes an immutable release, selects its programs and complete
documentation, and retains the superseded release through `previous`. Recovery
selects a verified retained release. Help/version checks load no credential and
read or send no mail. Stop on foreign selector ownership or failed verification;
do not overwrite another installation or execute an unverified retained installer.

## Deliver through Cell

1. Commit the intended candidate when commits and delivery are authorized.
2. Submit it from the Cell root:

   ```sh
   ./ci.sh submit COMMIT
   ```

   The installed alternative is `cell-ci submit COMMIT`.
3. Verify the retained manager outcome. The manager owns integration, validation,
   bounded repair, deployment, and its outcome email. Read
   `chancery show ci-manager.queue.operate` for the queue procedure.

## Install manually

1. Obtain explicit authority for manual installation or recovery. Select
   matching validated binary, installer, and bundle artifacts.
2. Run the tested installer:

   ```sh
   <TESTED_EMAIL_INSTALL> install \
     --binary <TESTED_EMAIL_BINARY> \
     --bundle <TESTED_EMAIL_BUNDLE>
   ```

3. Register the command inventory without mail access:

   ```sh
   email --register-usage
   ```

4. Read `email --help` and `email --version`. Check the installed publication
   separately when Chancery is available:

   ```sh
   chancery doctor
   chancery product email
   chancery show email.message.send
   chancery resolve email.install.operate
   ```

An explicit unspecified guarantee or uncontracted Resend reliance remains a
documentation gap, not evidence of a broken installation. Chancery reads do
not test live account access or authorize a send.

## Recover a retained program release

1. Resolve `~/Library/Application Support/Email/install/previous` to its
   canonical owned release directory. Stop when no verified owned release is available.
2. Run a trusted tested installer with that absolute directory:

   ```sh
   <TESTED_EMAIL_INSTALL> recover --release ABSOLUTE_RELEASE_DIRECTORY
   ```

3. Verify help/version and the selected provider publication. Register usage
   after selection with `email --register-usage`.

If a post-switch check fails, preserve the error and verify the restored
selectors. Do not infer mail or credential changes. Keep account settings
recovery separate; read `chancery show email.account.operate` when settings need
authorized correction.

## Configure access or verify a real send

1. Select an already provisioned sending-capable Resend credential and verified
   `joeytan.dev` sending domain. Remote provisioning requires separate authority.
2. Follow `chancery show email.account.operate` for private local credential and
   receiving-domain setup. The existing `RESEND_API_KEY` setting in the installed
   user's `~/.zshrc` remains a supported fallback; keep its bytes private.
3. Obtain separate authority for a harmless, uniquely identifiable real send
   and any disclosed content. Run:

   ```sh
   email 'Email CLI validation' 'The installed Email CLI can send through Resend.'
   ```

4. Verify `Accepted RESEND_MESSAGE_ID` and zero exit for Resend acceptance.
   Observe the intended message separately in Gmail when receipt is required.

For a requester-owned occurrence, use its stable idempotency key and exact
frozen payload under its existing authority. Read
`chancery show email.message.send` for disclosure and ambiguous-failure recovery.
Installer success never authorizes this verification send.
