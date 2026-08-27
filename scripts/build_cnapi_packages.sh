#!/usr/bin/env bash
# Build the branded Codex CLI for the host platform.
# Requires: rustup + target, Python 3, and network access for crates/V8.
# Usage:
#   NEWAPI_BASE_URL="https://newapi.example.com" ./scripts/build_cnapi_packages.sh aarch64-apple-darwin
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CODEX_REPO_ROOT="$REPO_ROOT"

export NEWAPI_PRODUCT_NAME="${NEWAPI_PRODUCT_NAME:-AHEIAPI}"
echo "Product name: $NEWAPI_PRODUCT_NAME"
export NEWAPI_BASE_URL="${NEWAPI_BASE_URL:-https://new.ahei.asia}"
echo "Baking in default New API base URL: $NEWAPI_BASE_URL"

DIST_DIR="${DIST_DIR:-$REPO_ROOT/dist}"
mkdir -p "$DIST_DIR"

for tool in cargo rustc python3; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 1
  fi
done

TARGETS=("$@")
if [[ ${#TARGETS[@]} -eq 0 ]]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) TARGETS=(aarch64-apple-darwin) ;;
    Darwin-x86_64) TARGETS=(x86_64-apple-darwin) ;;
    Linux-x86_64) TARGETS=(x86_64-unknown-linux-musl) ;;
    *) echo "No default target for this host; pass one explicitly." >&2; exit 1 ;;
  esac
fi

for target in "${TARGETS[@]}"; do
  case "$target" in
    *windows*) ext="zip" ;;
    *) ext="tar.gz" ;;
  esac
  safe="${target//[^A-Za-z0-9_.-]/_}"
  archive="$DIST_DIR/codex-$safe.$ext"
  echo "=== Building $target -> $archive ==="
  python3 "$REPO_ROOT/scripts/build_codex_package.py" \
    --variant codex \
    --target "$target" \
    --cargo-profile release \
    --archive-output "$archive" \
    --force
  test -f "$archive" || { echo "Expected archive not produced: $archive" >&2; exit 1; }
  echo "OK: $archive"
done

echo "All packages built:"
ls -lh "$DIST_DIR"
