#!/bin/bash
# Sync local changes to mini machine and build/install release binary
set -e

MINI_HOST="${MINI_HOST:-mini}"
MINI_PATH="${MINI_PATH:-/Users/wangyangzuo/tmp/opencli_rs_plus}"

echo "==> Pushing commits to origin..."
git push origin master

echo "==> Pulling updates on $MINI_HOST..."
ssh "$MINI_HOST" "cd '$MINI_PATH' && git pull --rebase"

echo "==> Building release on $MINI_HOST..."
ssh "$MINI_HOST" "cd '$MINI_PATH' && ./scripts/build-release.sh"

echo "==> Done! Release binary installed on $MINI_HOST."
