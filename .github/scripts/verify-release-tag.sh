#!/usr/bin/env bash
set -euo pipefail

tag="${1:?Release tag is required}"
expected_commit="${2:?Validated release commit is required}"
if [[ ! "$tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  echo "Invalid release tag: $tag" >&2
  exit 1
fi
if [[ ! "$expected_commit" =~ ^[0-9a-f]{40}$ ]]; then
  echo 'Validated release commit is not a full Git SHA.' >&2
  exit 1
fi

git fetch --force --no-tags origin \
  "+refs/tags/${tag}:refs/tags/${tag}"
actual_commit="$(git rev-parse "refs/tags/${tag}^{commit}")"
test "$actual_commit" = "$expected_commit" || {
  echo "Release tag moved after validation: $tag" >&2
  exit 1
}
