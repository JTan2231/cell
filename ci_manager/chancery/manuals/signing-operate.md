# Configure persistent macOS signing for Cell

Use `cell-ci signing` to inspect or explicitly select the signing identity for
Cell's native production executables. Cell selects one certificate by its exact
SHA-1 fingerprint. Builds, CI repairs, cache resets, reinstallations, and manager
upgrades do not select or generate another certificate.

This release supports the `local` signing profile. It adds no Hardened Runtime,
notarization, Developer ID distribution, installer-package signing, or macOS
permission grant. Signing continuity does not guarantee that macOS will never
request permission again.

## Inspect the selected identity

Run the installed command:

```sh
cell-ci signing status
```

Status reads the configuration and checks the exact certificate, its private
key, and code-signing usability through macOS Security and a dry-run signature.
It changes no signing configuration or product selection. A Keychain or macOS
authorization prompt can require the current user.

Success prints the selected fingerprint, namespace, Keychain, and readiness.
Add `--json` to receive `policy`, `policy_digest`, and `ready: true` as JSON:

```sh
cell-ci signing status --json
```

`ready` describes this probe; it is not a lasting readiness guarantee. Missing
configuration, an inaccessible Keychain, or an unavailable or expired identity
returns exit 78 with a diagnostic. CLI syntax errors return exit 2.

## Prepare an explicit configuration change

1. Pause CI admission with `cell-ci pause`.
2. Inspect `cell-ci status` and settle all active and queued jobs. Cancel queued
   jobs only when abandoning them is intended. A pause alone does not drain them.
3. Settle current and retained deployment recovery and release publication.
4. Unlock the intended Keychain and permit the requested key use through macOS.
5. Run one setup or configuration command below.
6. Inspect `cell-ci signing status` before resuming admission.

Configuration writers hold CI admission and deployment locks. They refuse an
unpaused queue, any active or queued job, a held release-publication lock, a live
deployment lock, or retained deployment recovery. No configuration mutation
cancels work or removes a lock or journal.

The setup command requires macOS, the configured external Cell work volume,
current-user Keychain access, `/usr/bin/security`, `/usr/bin/codesign`, and
`/usr/bin/openssl`. The configuration file is owned by the current user and
has mode 0600. Its parent directory has mode 0700.

Before the updated manager is installed, use the source client from the Cell
root with the same subcommands:

```sh
python3 ci_manager/client.py signing status
python3 ci_manager/client.py signing create-local
```

`./ci.sh signing` routes to the installed manager. It does not select edited
source automatically. Read `ci-manager.queue.operate` before manager replacement.

## Create the initial local certificate

Run this command only for initial setup:

```sh
cell-ci signing create-local
```

The command creates one self-signed `Cell Local Signing` certificate with an
RSA 3072-bit private key, a SHA-256 certificate signature, 3,650-day validity,
digital-signature key usage, and code-signing extended key usage. It imports
the certificate and key into the current user's login Keychain, authorizes
`/usr/bin/codesign` for that key, and adds user code-signing trust for the
certificate. macOS can require native user authorization for those actions.

The command preflights the identity before writing signing configuration.
It refuses existing signing configuration or a `Cell Local Signing` certificate
in the selected login Keychain. Initial creation does not renew a certificate
or replace an unavailable identity.

Generation files remain in private temporary staging and are removed when setup
exits. Cell retains the certificate and private key in Keychain. It retains no
certificate or private-key export in the host filesystem.

Success prints the selected fingerprint, namespace, and Keychain. Add `--json`
to receive the selected `policy` and `policy_digest`. A failure can leave an
imported certificate without configuration. Use `configure` to select that
existing identity after its key is usable. Do not delete it to rerun creation.

## Select an existing certificate

Run this command with the intended certificate's exact fingerprint and Keychain:

```sh
cell-ci signing configure \
  --certificate-sha1 FINGERPRINT \
  --keychain /absolute/login.keychain-db \
  --identifier-namespace local.cell
```

`FINGERPRINT` has exactly 40 hexadecimal digits; Cell normalizes it to lowercase.
The Keychain path must be absolute and name a regular file owned by the current
user. The namespace defaults to `local.cell` and must be a dotted lowercase
identifier. Certificate name matching, per-product overrides, and environment
signer overrides are unsupported.

Configuration checks the selected identity before atomically writing
`~/Library/Application Support/Cell/signing.json`. Success prints the selected
fingerprint, namespace, and Keychain. Add `--json` to receive `policy` and
`policy_digest`. Failure preserves prior configuration. The supported format is:

```json
{
  "schema": 1,
  "macos": {
    "profile": "local",
    "certificate_sha1": "<40 lowercase hexadecimal digits>",
    "keychain": "/absolute/login.keychain-db",
    "identifier_namespace": "local.cell"
  }
}
```

The fingerprint selects the certificate, not the executable-content hash
algorithm. The policy digest is SHA-256 of normalized canonical policy JSON.
The configuration contains no private-key bytes or password.

Changing the fingerprint explicitly selects another certificate. Changing the
namespace explicitly changes all product code identifiers. Changing only the
Keychain path locates the same selected certificate in another Keychain.

## Apply and verify the policy

New macOS jobs freeze the policy and digest at submission. A reused request key
keeps its original snapshot. Retained jobs without a signing snapshot keep
their earlier path; installation does not invent one for them.

For each product, the permanent executable key is its declared release-binary
name. The identifier is `NAMESPACE.PRODUCT.EXECUTABLE`, using canonical product
identity `krisis` for the former `decisions` descriptor. Source versions, build
paths, Git commits, and release directories do not enter this identifier.

The designated requirement pins that identifier and the exact leaf certificate.
Cell signs staged native code, verifies it against the selected requirement,
then hashes and seals the signed bytes. It uses no network timestamp and applies
no new entitlements or Hardened Runtime options. Products that require additional
signing inputs need an explicit policy extension.

The production cache includes signing inputs. Cache reuse verifies stored
artifact hashes and signatures. CI retains a production receipt and verifies
signed candidates before accepted-ref advancement, including jobs that skip
tests. Deployment and release preparation apply the same configured policy.
Policy changes detected after admission stop publication.

Coverage is Cell-owned native production commands, daemons, helpers, and
installer executables declared in the product inventory. Standalone scripts,
the Python CI manager, test executables, compiler scratch output, and third-party
runtimes do not receive a Cell-native process identity through this policy.
Scripts and other package assets remain covered by their product release
manifests and hashes. Signing does not combine products' permission boundaries.

Missing keys, expired certificates, locked Keychains, wrong signers, or failed
verification stop the operation. There is no unsigned, ad hoc, alternate-name,
certificate-renewal, or automatically generated identity fallback. Signing
configuration failures do not request model repair. The manager supplies
repair agents no private-key material or signing configuration writer.

## Make a deliberate identity change

If the selected certificate or private key is unavailable, signing remains
blocked until that identity is usable or the user explicitly configures another
identity. Cell does not generate a replacement. Reissuing a certificate with
the same private key still changes the fingerprint and requires explicit
selection.

For initial adoption or rotation, select the full intended product inventory
in CI with repeated `--deploy PRODUCT` options and verify the retained deployment
outcome. Ordinary selective CI updates only its selected products. Install the
manager separately under its maintenance procedure; product deployment does
not replace the manager.

Product installation is sequential. A failed rollout can leave prior signed
releases selected. Inspect each retained
outcome before claiming that the full inventory uses one signer. Existing
immutable releases are never re-signed in place. Prepare historical source as
a new release under the current policy.

No certificate auto-renewal, fleet-wide atomic rotation, historical release
retro-signing, private-key escrow, or completion-time guarantee is supplied.
The ten-year certificate lifetime is a creation setting, not a perpetual
usability promise. The current-user authority boundary does not isolate the key
from arbitrary hostile code already running with that user's authority.
