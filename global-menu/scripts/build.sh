#!/usr/bin/env bash
# Build the bridge and copy the binary into the plugin directory.
set -euo pipefail
cd "$(dirname "$0")/../bridge"
cargo build --release
mkdir -p ../bridge-bin
cp target/release/noctalia-global-menu-bridge ../bridge-bin/
echo "bridge installed to global-menu/bridge-bin/"
