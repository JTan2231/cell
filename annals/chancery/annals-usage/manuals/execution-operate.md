# Inspect the Annals execution path and account allowance

Annals Usage provides Annals-facing account and diagnostic commands while
Nucleus remains the sole execution and credential authority.

## Live allowance

```sh
/Users/joey/.local/bin/annals-usage budget
```

Budget makes live Nucleus account rate-limit and activity reads. It reports the
account plan, available allowance windows, used percentages, reset times, and
credit fields at one observation time. Lifetime, peak-day, and latest-day token
activity are contextual cross-checks; they are not allowance units.

The backend does not expose a token denominator for a subscription window or a
per-delivery percentage. Other Codex work shares the account. Do not infer
those values from repeated snapshots or call the result Annals-specific
consumption. Use `annals-usage report` for delivery-attributed model tokens.

Nothing from budget is retained by Annals Usage.

## Diagnostic

```sh
/Users/joey/.local/bin/annals-usage doctor
```

Doctor checks the companion configuration, selected Annals library and spool,
Nucleus and Codex versions, strict readiness, and authenticated account
telemetry access. It creates no state. A failure identifies the current
configuration, filesystem, runtime, compatibility, or account boundary; it is
not an Annals corpus-success diagnosis.

Budget and doctor use a nonblocking canonical-credential request. An
authentication-busy result means a canonical account, refresh, or login
operation owns that boundary and should be allowed to finish; an active job
alone does not make the request busy or prove invalid credentials.

## Attended login

When authentication recovery is actually required:

1. Prevent new requester work and let active attempts settle.
2. If scheduled Annals dispatch is active, pause it and wait for the current
   delivery to finish.
3. Run:

   ```sh
   /Users/joey/.local/bin/annals-usage login --device-auth
   /Users/joey/.local/bin/annals-usage doctor
   ```

4. Run a deliberate Annals integration canary and inspect Annals domain state.
5. Resume only the pause established for this recovery.

Login delegates to `nucleus auth login --device-auth`. Annals and Annals Usage
never read, copy, configure, or retain credential files. Credential state is
forward-only and must not be silently restored by binary or database rollback.

Account responses and activity are private account information. The companion
CLI retains neither them nor credentials.
