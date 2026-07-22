#!/usr/bin/env bash
set -euo pipefail

required=(
  TAURI_SIGNING_PRIVATE_KEY
  WINDOWS_CERTIFICATE
  WINDOWS_CERTIFICATE_PASSWORD
  APPLE_CERTIFICATE
  APPLE_CERTIFICATE_PASSWORD
  APPLE_ID
  APPLE_PASSWORD
  APPLE_TEAM_ID
)
missing=()
for variable_name in "${required[@]}"; do
  if test -z "${!variable_name:-}"; then
    missing+=("${variable_name}")
  fi
done

if test "${#missing[@]}" -ne 0; then
  printf 'Missing required release-signing secret: %s\n' "${missing[@]}" >&2
  exit 1
fi
