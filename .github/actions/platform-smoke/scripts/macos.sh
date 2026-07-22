#!/usr/bin/env bash
set -euo pipefail

bundle_directory="${1:?macOS bundle directory is required}"
test -d "${bundle_directory}"
dmg="$(find "${bundle_directory}" -name '*.dmg' -print -quit)"
test -n "${dmg}"

home="$(mktemp -d "${RUNNER_TEMP}/openquota-macos-home.XXXXXX")"
mount_dir="$(mktemp -d "${RUNNER_TEMP}/openquota-macos-dmg.XXXXXX")"
log="${RUNNER_TEMP}/openquota-macos-${RANDOM}.log"
app_pid=''
mounted=false
cleanup() {
  if test -n "${app_pid}"; then
    kill "${app_pid}" 2>/dev/null || true
    wait "${app_pid}" 2>/dev/null || true
  fi
  if test "${mounted}" = true; then
    hdiutil detach "${mount_dir}" -force >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

export HOME="${home}"
hdiutil attach "${dmg}" -mountpoint "${mount_dir}" -nobrowse -readonly >/dev/null
mounted=true
app="${mount_dir}/OpenQuota.app"
binary="${mount_dir}/OpenQuota.app/Contents/MacOS/openquota"
test -x "${binary}"
if test "${OPENQUOTA_REQUIRE_NOTARIZATION:-false}" = true; then
  codesign --verify --deep --strict --verbose=2 "${app}"
  spctl --assess --type execute --verbose=2 "${app}"
  xcrun stapler validate "${app}"
fi
"${binary}" >"${log}" 2>&1 &
app_pid=$!
sleep 8
if ! kill -0 "${app_pid}" 2>/dev/null; then
  cat "${log}"
  exit 1
fi
