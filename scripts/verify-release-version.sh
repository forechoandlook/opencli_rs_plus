#!/usr/bin/env bash
set -euo pipefail

tag="${1:-${GITHUB_REF_NAME:-}}"
if [[ -z "$tag" ]]; then
  echo "missing release tag" >&2
  exit 1
fi

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
expected="v${version}"

if [[ "$tag" != "$expected" ]]; then
  echo "tag/version mismatch: tag=$tag expected=$expected" >&2
  exit 1
fi

echo "release version check passed: $tag"
