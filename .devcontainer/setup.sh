#!/bin/bash
set -e

cargo build -p gabagool-debug-adapter

mkdir -p gabagool-debug-adapter/bin
cp target/debug/gabagool-debug-adapter gabagool-debug-adapter/bin/

npm install -g @vscode/vsce
cd gabagool-debug-adapter
echo y | vsce package -o gabagool-debug.vsix
cd ..
