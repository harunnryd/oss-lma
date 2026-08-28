
#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE="${1:-$REPO_ROOT/target/release/bundle/macos/oss-lma.app}"
IDENTITY="${LMA_CODESIGN_IDENTITY:--}"

if [[ ! -d "$BUNDLE" ]]; then
  echo "error: bundle not found at $BUNDLE" >&2
  echo "hint:  run \`cargo tauri build\` first, or pass a path" >&2
  exit 1
fi

echo "→ signing bundle with identity: $IDENTITY"
codesign --force --sign "$IDENTITY" "$BUNDLE"

echo "→ verifying"
codesign --verify --deep --strict --verbose=2 "$BUNDLE"

echo
echo "✓ signature is valid"
