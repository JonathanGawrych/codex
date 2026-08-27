#!/bin/bash

set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)

SOURCE_STATUS=$(git -C "$REPO_ROOT" status --porcelain --untracked-files=normal)
if [[ -n "$SOURCE_STATUS" ]]; then
  echo "Codex source checkout has uncommitted changes:" >&2
  echo "$SOURCE_STATUS" >&2
  echo "Commit or remove them before running the update." >&2
  exit 1
fi

echo "Fetching origin/main..."
git -C "$REPO_ROOT" fetch --prune origin

echo "Rebasing $(git -C "$REPO_ROOT" branch --show-current) onto origin/main..."
git -C "$REPO_ROOT" rebase origin/main

echo "Building the release binary..."
cd "$REPO_ROOT/codex-rs"
cargo build --release -p codex-cli
