#!/bin/sh
# Build the local release binary and make it immediately available on PATH.
# Usage: ./scripts/build-release.sh [additional cargo build arguments]

set -eu

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TARGET_DIR=${CARGO_TARGET_DIR:-"$REPO_ROOT/target"}
case "$TARGET_DIR" in
    /*) ;;
    *) TARGET_DIR="$REPO_ROOT/$TARGET_DIR" ;;
esac

cargo build --manifest-path "$REPO_ROOT/Cargo.toml" --release --bin opencli "$@"

SOURCE_BINARY="$TARGET_DIR/release/opencli"
INSTALL_DIR="$HOME/.local/bin"
DESTINATION="$INSTALL_DIR/opencli"

if [ ! -x "$SOURCE_BINARY" ]; then
    echo "Release binary was not produced: $SOURCE_BINARY" >&2
    exit 1
fi

mkdir -p "$INSTALL_DIR"
install -m 755 "$SOURCE_BINARY" "$DESTINATION"

echo "Installed release binary: $DESTINATION"
"$DESTINATION" --version
