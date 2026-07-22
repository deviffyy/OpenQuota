#!/usr/bin/env bash
set -euo pipefail

appimage_directory="$(realpath "${1:?AppImage directory is required}")"
deb_directory="$(realpath "${2:?Debian package directory is required}")"
script_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

mapfile -t appimages < <(find "${appimage_directory}" -maxdepth 1 -type f -name '*.AppImage' -print)
mapfile -t debs < <(find "${deb_directory}" -maxdepth 1 -type f -name '*.deb' -print)
test "${#appimages[@]}" -eq 1 || {
  echo "Expected exactly one AppImage, found ${#appimages[@]}." >&2
  exit 1
}
test "${#debs[@]}" -eq 1 || {
  echo "Expected exactly one Debian package, found ${#debs[@]}." >&2
  exit 1
}

appimage="${appimages[0]}"
deb="${debs[0]}"
chmod +x "${appimage}"

# Running with the AppImage runtime's extraction mode exercises the published
# container without depending on FUSE being enabled on hosted runners.
export APPIMAGE_EXTRACT_AND_RUN=1
bash "${script_directory}/linux-x11.sh" "${appimage}"
unset APPIMAGE_EXTRACT_AND_RUN

package_name="$(dpkg-deb --field "${deb}" Package)"
package_architecture="$(dpkg-deb --field "${deb}" Architecture)"
runner_architecture="$(dpkg --print-architecture)"
test "${package_name}" = 'open-quota' || {
  echo "Unexpected Debian package name: ${package_name}" >&2
  exit 1
}
test "${package_architecture}" = "${runner_architecture}" || {
  echo "Debian package architecture ${package_architecture} does not match runner ${runner_architecture}." >&2
  exit 1
}
if dpkg-query --show --showformat='${db:Status-Status}' "${package_name}" 2>/dev/null \
  | grep -Fxq installed; then
  echo "Refusing to replace an existing ${package_name} installation on the runner." >&2
  exit 1
fi

installed=false
cleanup() {
  if test "${installed}" = true; then
    sudo apt-get remove --yes "${package_name}" >/dev/null
  fi
}
trap cleanup EXIT

sudo apt-get install --yes "${deb}"
installed=true
installed_binary="$(dpkg-query --listfiles "${package_name}" | grep -E '/(usr/)?bin/openquota$' | head -n 1)"
test -n "${installed_binary}"
test -x "${installed_binary}"
bash "${script_directory}/linux-wayland.sh" "${installed_binary}"
