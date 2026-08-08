#!/usr/bin/env bash
set -euo pipefail

linuxdeploy_release='1-alpha-20251107-1'

case "$(uname -m)" in
  x86_64)
    linuxdeploy_arch='x86_64'
    linuxdeploy_sha256='c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d'
    ;;
  aarch64)
    linuxdeploy_arch='aarch64'
    linuxdeploy_sha256='620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff'
    ;;
  *)
    echo "Unsupported linuxdeploy host architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

tauri_cache_directory="${XDG_CACHE_HOME:-${HOME}/.cache}/tauri"
linuxdeploy_path="${tauri_cache_directory}/linuxdeploy-${linuxdeploy_arch}.AppImage"
download_path="$(mktemp "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/linuxdeploy.XXXXXX")"
trap 'rm -f "${download_path}"' EXIT

curl --fail --location --silent --show-error --retry 3 \
  --output "${download_path}" \
  "https://github.com/linuxdeploy/linuxdeploy/releases/download/${linuxdeploy_release}/linuxdeploy-${linuxdeploy_arch}.AppImage"
echo "${linuxdeploy_sha256}  ${download_path}" | sha256sum --check --strict

mkdir -p "${tauri_cache_directory}"
install -m 0755 "${download_path}" "${linuxdeploy_path}"
