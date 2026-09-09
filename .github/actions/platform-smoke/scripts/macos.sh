#!/usr/bin/env bash
set -euo pipefail

bundle_directory="${1:?macOS bundle directory is required}"
release_validation="${2:-false}"
case "${release_validation}" in
  true | false) ;;
  *)
    echo "macOS release validation must be true or false: ${release_validation}" >&2
    exit 1
    ;;
esac
if test "${release_validation}" = true \
  && test -z "${OPENQUOTA_EXPECTED_APPLE_TEAM_ID:-}"; then
  echo 'OPENQUOTA_EXPECTED_APPLE_TEAM_ID is required for release validation.' >&2
  exit 1
fi

verify_app_signature() {
  local candidate="${1:?macOS application bundle is required}"
  codesign --verify --deep --strict --verbose=2 "${candidate}"
}

verify_release_trust() {
  local candidate="${1:?macOS application bundle is required}"
  local details

  details="$(codesign -dv --verbose=4 "${candidate}" 2>&1)"
  grep -Fq 'Authority=Developer ID Application:' <<<"${details}" || {
    echo "${details}" >&2
    echo 'The macOS app is not signed with a Developer ID Application identity.' >&2
    return 1
  }
  grep -Fq "TeamIdentifier=${OPENQUOTA_EXPECTED_APPLE_TEAM_ID}" <<<"${details}" || {
    echo "${details}" >&2
    echo 'The macOS app has an unexpected Developer ID team.' >&2
    return 1
  }
  grep -Eq 'flags=0x[0-9a-fA-F]+\([^)]*runtime[^)]*\)' <<<"${details}" || {
    echo "${details}" >&2
    echo 'The macOS app does not enable the hardened runtime.' >&2
    return 1
  }
  grep -Fq 'Timestamp=' <<<"${details}" || {
    echo "${details}" >&2
    echo 'The macOS app signature does not have a secure timestamp.' >&2
    return 1
  }
  spctl --assess --type execute --verbose=2 "${candidate}"
  xcrun stapler validate "${candidate}"
}

test -d "${bundle_directory}"
dmgs=()
while IFS= read -r candidate; do
  dmgs+=("${candidate}")
done < <(find "${bundle_directory}" -maxdepth 1 -type f -name '*.dmg' -print)
test "${#dmgs[@]}" -eq 1 || {
  echo "Expected exactly one macOS DMG, found ${#dmgs[@]}." >&2
  exit 1
}
dmg="${dmgs[0]}"

runner_temp="${RUNNER_TEMP:?RUNNER_TEMP is required for the macOS package smoke test}"
mount_dir="$(mktemp -d "${runner_temp}/openquota-macos-dmg.XXXXXX")"
install_root="$(mktemp -d "${runner_temp}/openquota-macos-install.XXXXXX")"
app="${install_root}/Applications/OpenQuota.app"
binary="${app}/Contents/MacOS/openquota"
launch_log="${runner_temp}/openquota-macos-${RANDOM}.log"
app_log="${HOME}/Library/Logs/OpenQuota/OpenQuota.log"
app_pid=''
mounted=false
cleanup() {
  if test -n "${app_pid}"; then
    kill "${app_pid}" 2>/dev/null || true
    wait "${app_pid}" 2>/dev/null || true
  elif test -n "${binary}"; then
    pkill -f "${binary}" 2>/dev/null || true
  fi
  if test "${mounted}" = true; then
    hdiutil detach "${mount_dir}" -force >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if pgrep -x openquota >/dev/null 2>&1; then
  echo 'Refusing to disturb an existing OpenQuota process on the macOS runner.' >&2
  exit 1
fi
if test -e "${app_log}"; then
  echo "Refusing to use an existing OpenQuota log for the macOS smoke test: ${app_log}" >&2
  exit 1
fi

if test "${release_validation}" = true; then
  quarantine_value="0081;$(printf '%x' "$(date +%s)");OpenQuotaSmoke;"
  xattr -w com.apple.quarantine "${quarantine_value}" "${dmg}"
  codesign --verify --strict --verbose=2 "${dmg}"
  dmg_details="$(codesign -dv --verbose=4 "${dmg}" 2>&1)"
  grep -Fq 'Authority=Developer ID Application:' <<<"${dmg_details}" || {
    echo "${dmg_details}" >&2
    echo 'The macOS DMG is not signed with a Developer ID Application identity.' >&2
    exit 1
  }
  grep -Fq "TeamIdentifier=${OPENQUOTA_EXPECTED_APPLE_TEAM_ID}" <<<"${dmg_details}" || {
    echo "${dmg_details}" >&2
    echo 'The macOS DMG has an unexpected Developer ID team.' >&2
    exit 1
  }
  grep -Fq 'Timestamp=' <<<"${dmg_details}" || {
    echo "${dmg_details}" >&2
    echo 'The macOS DMG signature does not have a secure timestamp.' >&2
    exit 1
  }
fi

hdiutil attach "${dmg}" -mountpoint "${mount_dir}" -nobrowse -readonly >/dev/null
mounted=true
source_app="${mount_dir}/OpenQuota.app"
source_binary="${source_app}/Contents/MacOS/openquota"
test -x "${source_binary}"
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleDevelopmentRegion' "${source_app}/Contents/Info.plist")" = en
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleLocalizations:0' "${source_app}/Contents/Info.plist")" = en
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleLocalizations:1' "${source_app}/Contents/Info.plist")" = zh-Hans
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleLocalizations:2' "${source_app}/Contents/Info.plist")" = zh-Hant
verify_app_signature "${source_app}"
if test "${release_validation}" = true; then
  verify_release_trust "${source_app}"
fi

mkdir -p "$(dirname "${app}")"
ditto "${source_app}" "${app}"
hdiutil detach "${mount_dir}" >/dev/null
mounted=false
test -x "${binary}"
verify_app_signature "${app}"

if test "${release_validation}" = true; then
  verify_release_trust "${app}"
  if command -v syspolicy_check >/dev/null 2>&1; then
    syspolicy_check distribution "${app}"
  fi
  xattr -w com.apple.quarantine "${quarantine_value}" "${app}"
  xattr -p com.apple.quarantine "${app}" >/dev/null
  spctl --assess --type execute --verbose=2 "${app}"
  xattr -d com.apple.quarantine "${app}"
fi

open -n "${app}" >"${launch_log}" 2>&1
tray_ready=false
startup_complete=false
for _ in $(seq 1 30); do
  app_pid="$(pgrep -f "${binary}" | head -n 1 || true)"
  if test -f "${app_log}"; then
    grep -Fq 'system tray integration ready' "${app_log}" && tray_ready=true
    grep -Fq 'OpenQuota startup completed' "${app_log}" && startup_complete=true
  fi
  if test -n "${app_pid}" && test "${tray_ready}" = true && test "${startup_complete}" = true; then
    break
  fi
  sleep 1
done
if test -z "${app_pid}" || test "${tray_ready}" != true || test "${startup_complete}" != true; then
  cat "${launch_log}" >&2 || true
  cat "${app_log}" >&2 || true
  echo 'OpenQuota did not report a ready macOS tray before the startup deadline.' >&2
  exit 1
fi
