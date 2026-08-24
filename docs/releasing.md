# Releasing OpenQuota

OpenQuota treats updater signatures and native operating-system signatures as separate trust
layers. Updater artifacts must always be signed with `TAURI_SIGNING_PRIVATE_KEY`. Native Windows
and macOS signing are independent opt-ins because they require externally provisioned certificates.
If the updater key is encrypted, also configure `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The updater
key is project-generated and does not require a paid certificate authority or signing account.

## Default release policy

Leave both native-signing repository variables unset or set them to `false`:

- `ENABLE_WINDOWS_NATIVE_SIGNING`
- `ENABLE_MACOS_NATIVE_SIGNING`

The release workflow then builds an unsigned Windows installer and an ad-hoc-signed, unnotarized
macOS application. Package installation and startup smoke tests still run, but Authenticode,
Gatekeeper, and notarization checks are skipped. The workflow emits warnings, and the download
documentation describes the unavailable native trust layers.

This default does not weaken updater verification. Tauri updater signatures are still generated,
uploaded, and verified with the bundled public key before publication.

## Enabling Windows native signing

Set `ENABLE_WINDOWS_NATIVE_SIGNING` to `true` only after configuring all of the following:

| Kind             | Name                     |
| ---------------- | ------------------------ |
| Actions secret   | `ES_USERNAME`            |
| Actions secret   | `ES_PASSWORD`            |
| Actions secret   | `ES_CREDENTIAL_ID`       |
| Actions secret   | `ES_TOTP_SECRET`         |
| Actions variable | `WINDOWS_SIGNER_SUBJECT` |

This enables the reviewed SSL.com CodeSignTool configuration. The workflow then requires a valid,
timestamped Authenticode signature on both the installer and installed executable. Missing or
incorrect values stop the release rather than silently producing an unsigned Windows artifact.

## Enabling macOS native signing

Set `ENABLE_MACOS_NATIVE_SIGNING` to `true` only after configuring all of the following Actions
secrets:

- `APPLE_CERTIFICATE`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_ID`
- `APPLE_PASSWORD`
- `APPLE_TEAM_ID`

`APPLE_PASSWORD` is an app-specific password used for notarization, not the account's normal login
password.

The direct-download DMG uses a Developer ID Application certificate, not an App Store Distribution
certificate. When enabled, the workflow requires the expected team identity, hardened runtime,
secure timestamp, Gatekeeper approval, and a valid notarization staple. Missing or incorrect values
stop the release rather than falling back to ad-hoc signing.

Both opt-ins accept only the exact strings `true` and `false`. An invalid value stops validation so a
typo cannot silently change release trust policy. A `verify_only` run publishes an already-built
draft and therefore does not require private signing credentials. The two policy variables must
still match the draft's native-signing state; a mismatch stops publication.
