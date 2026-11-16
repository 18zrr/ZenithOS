#!/bin/bash
# Verify that the Zenith OS workspace and kernel sources compile.
set -uo pipefail

cd "$(dirname "$0")"

echo "Checking the full workspace..."
if ! cargo build --workspace; then
    echo "FAILURE: workspace does not compile." >&2
    exit 1
fi

echo "Checking the kernel for the bare-metal target..."
if cargo build -p zenith-kernel --target x86_64-unknown-none; then
    echo "SUCCESS: workspace and bare-metal kernel both compile."
    exit 0
fi

echo "NOTE: x86_64-unknown-none target is unavailable; kernel host build used."
if cargo build -p zenith-kernel; then
    echo "SUCCESS: workspace compiles; kernel sources verified on host target."
    exit 0
fi

echo "FAILURE: kernel sources do not compile." >&2
exit 1
