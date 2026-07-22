#!/usr/bin/env bash
set -euo pipefail

for variable_name in \
  APPLE_CERTIFICATE \
  APPLE_CERTIFICATE_PASSWORD \
  APPLE_ID \
  APPLE_PASSWORD \
  APPLE_TEAM_ID; do
  test -n "${!variable_name:-}" || {
    echo "Missing required Apple signing secret: ${variable_name}" >&2
    exit 1
  }
done

certificate_path="${RUNNER_TEMP}/openquota-certificate.p12"
keychain_path="${RUNNER_TEMP}/openquota-signing.keychain-db"
keychain_password="$(openssl rand -hex 24)"
trap 'rm -f "$certificate_path"' EXIT

printf '%s' "$APPLE_CERTIFICATE" | tr -d '\r\n' | openssl base64 -d -A > "$certificate_path"
security create-keychain -p "$keychain_password" "$keychain_path"
echo "OPENQUOTA_APPLE_KEYCHAIN=$keychain_path" >> "$GITHUB_ENV"
security set-keychain-settings -lut 21600 "$keychain_path"
security unlock-keychain -p "$keychain_password" "$keychain_path"
security import "$certificate_path" \
  -k "$keychain_path" \
  -P "$APPLE_CERTIFICATE_PASSWORD" \
  -T /usr/bin/codesign
security set-key-partition-list \
  -S apple-tool:,apple: \
  -s \
  -k "$keychain_password" \
  "$keychain_path"
existing_keychains=()
while IFS= read -r existing_keychain; do
  existing_keychains+=("$existing_keychain")
done < <(security list-keychains -d user | sed -E 's/^[[:space:]]*"//; s/"[[:space:]]*$//')
security list-keychains -d user -s "$keychain_path" "${existing_keychains[@]}"

identity="$(
  security find-identity -v -p codesigning "$keychain_path" \
    | sed -n 's/.*"\(Developer ID Application:.*\)"/\1/p' \
    | head -n 1
)"
test -n "$identity" || {
  echo 'A Developer ID Application identity was not found in APPLE_CERTIFICATE.' >&2
  exit 1
}

echo "APPLE_SIGNING_IDENTITY=$identity" >> "$GITHUB_ENV"
echo 'OPENQUOTA_REQUIRE_NOTARIZATION=true' >> "$GITHUB_ENV"
