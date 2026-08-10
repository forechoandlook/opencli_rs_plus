#!/usr/bin/env bash
# Push opencli-rs/adapters into a checkout of opencli-adapters (plugin distribution repo).
set -euo pipefail

SRC_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-}"

if [[ -z "$DEST" ]]; then
  if [[ -d "$SRC_ROOT/../opencli-adapters" ]]; then
    DEST="$SRC_ROOT/../opencli-adapters"
  else
    echo "Usage: $0 /path/to/opencli-adapters" >&2
    exit 1
  fi
fi

if [[ ! -f "$DEST/opencli-plugin.json" ]]; then
  echo "Destination does not look like opencli-adapters (missing opencli-plugin.json): $DEST" >&2
  exit 1
fi

ADAPTERS="$SRC_ROOT/adapters"
echo "Syncing $ADAPTERS -> $DEST/"

# Drop previous site packages; keep plugin meta / scripts / git
find "$DEST" -mindepth 1 -maxdepth 1 -type d ! -name '.git' ! -name 'scripts' -exec rm -rf {} +
rsync -a \
  --exclude '.DS_Store' \
  --exclude 'cache.json' \
  "$ADAPTERS/" "$DEST/"

count=$(find "$DEST" -name '*.yaml' | wc -l | tr -d ' ')
echo "Done: $count yaml files written under $DEST"
echo "Next: bump version in opencli-plugin.json, CHANGELOG, commit & tag in opencli-adapters."
