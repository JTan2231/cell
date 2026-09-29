# Install or recover Conversations

Use this operation for an explicitly authorized manual installation or recovery
from validated artifacts. Ordinary delivery uses `cell-ci submit COMMIT` and
the installed manager. This procedure does not authorize release publication,
CI submission, task mutation, transcript disclosure, or Codex authentication changes.

Read `conversations.runtime` for installed paths, content identity, lock and
selector behavior, failure recovery, and the embedded-consumer boundary.
Read `nucleus manual` for shared maintenance and coordination.

## Install validated artifacts

1. Select the matching tested binary, Rust installer, and complete provider bundle.
2. Confirm authority for the installation and the expected current selection.
3. Install the validated candidate:

   ```sh
   <TESTED_CONVERSATIONS_INSTALL> install \
     --binary <TESTED_CONVERSATIONS_BINARY> \
     --bundle <TESTED_CONVERSATIONS_BUNDLE>
   ```

4. Register command usage inventory:

   ```sh
   conversations --register-usage
   ```

5. Check the selected path, version, and App Server handshake in the operator's normal environment:

   ```sh
   conversations doctor
   chancery product conversations
   chancery show conversations.history.explore
   chancery resolve conversations.installation.operate
   ```

Installation changes the binary and provider selectors. It starts no service,
imports no credential, and reads no transcript. Registration records command
inventory without reading history. The separate doctor check reads metadata
without repair or turn content. Chancery reads only installed documentation;
a declared upstream reliance or unspecified promise remains a documented gap.

Use `--expected-current absent` for an explicit fresh-install guard or
`--expected-current releases/HASH` for an explicit update guard. Stop on a
stale selection, foreign selector, invalid retained release, or unverified
restoration. Do not force selectors or bypass integrity checks. Preserve failure
diagnostics and inspect the retained selection before retrying deliberately.

## Recover a retained release

1. Resolve `install/previous` to its canonical owned release directory under the product installation root documented in `conversations.runtime`.
2. Select a trusted tested Rust installer.
3. Recover the verified release:

   ```sh
   <TESTED_CONVERSATIONS_INSTALL> recover --release ABSOLUTE_RELEASE_DIRECTORY
   ```

4. Register usage and repeat the doctor and installed-document checks above.

Recovery changes selectors after complete retained-release verification.
It supports the previous shell-installed format and `cell-install-v2`.
It does not run a retained installer to verify the release. Keep the candidate
and diagnostics when verification or restoration fails. Stop if ownership or
integrity is unknown. Do not remove or replace a foreign path.

## Verify completion

Confirm the intended CLI and provider bundle are selected together. Verify the
expected release and the separate App Server handshake. An installed bundle
alone does not prove runtime compatibility. Rebuild and deploy affected embedded
consumers when library behavior changed; replacing the CLI does not update them.

## Privacy and command usage

Keep diagnostics private; App Server stderr can contain operational context.
No real transcript or credential belongs in a provider bundle or example.
CLI usage recording requires a nonempty `CODEX_THREAD_ID`. Chancery's private
journal records command identity, time, and thread ID, not arguments, output,
or outcomes. Internal product calls are excluded. Recording errors do not
change command results.
