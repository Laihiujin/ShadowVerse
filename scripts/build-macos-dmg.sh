#!/usr/bin/env bash

set -euo pipefail

TARGET="${1:?target is required}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

export SDKROOT="${SDKROOT:-$(xcrun --sdk macosx --show-sdk-path)}"
export CMAKE_OSX_DEPLOYMENT_TARGET="${CMAKE_OSX_DEPLOYMENT_TARGET:-13.3}"

if [[ "$TARGET" == "aarch64-apple-darwin" ]]; then
  export GGML_METAL_EMBED_LIBRARY="${GGML_METAL_EMBED_LIBRARY:-1}"
fi

yarn tauri build --target "$TARGET" --bundles app
bash ./scripts/package-macos-dmg.sh "$TARGET"
