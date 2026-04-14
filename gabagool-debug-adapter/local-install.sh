#!/bin/bash
set -e

cd "$(dirname "$0")/.."

cargo build -p gabagool-debug-adapter

mkdir -p gabagool-debug-adapter/bin
cp target/debug/gabagool-debug-adapter gabagool-debug-adapter/bin/

echo "done — reload VS Code window"
