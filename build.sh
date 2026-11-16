#!/bin/bash
# Build all Zenith OS userland applications in one workspace build.
set -euo pipefail

cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
    echo "Error: Cargo (Rust build tool) is not installed" >&2
    exit 1
fi

echo "Building Zenith OS applications..."
cargo build --release \
    -p filemanager \
    -p systemsettings \
    -p calculator \
    -p browser \
    -p mediaplayer \
    -p systemmonitor \
    -p terminal

echo "All applications built successfully."
