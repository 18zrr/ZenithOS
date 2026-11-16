#!/bin/bash
# Build the Zenith OS kernel library and, when the bare-metal target is
# available, link it with the assembly entry point into kernel.bin.
set -euo pipefail

cd "$(dirname "$0")"

TARGET=x86_64-unknown-none

echo "Building kernel library for $TARGET..."
if cargo build --target="$TARGET" --release; then
    make build
    echo "SUCCESS: kernel.bin linked."
    exit 0
fi

echo "WARNING: $TARGET target unavailable; running host source verification." >&2
if cargo build --release; then
    echo "SOURCE CHECK PASSED: no bootable image was produced." >&2
    exit 0
fi

echo "FAILURE: kernel sources do not compile." >&2
exit 1
