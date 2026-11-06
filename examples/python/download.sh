#!/bin/sh
set -eu

example_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
archive=python-3.13.11-wasi_sdk-24.zip
url="https://github.com/brettcannon/cpython-wasi-build/releases/download/v3.13.11/$archive"
expected_sha256=e99a617738ade87cd263aa46cace7173faa91b5de994499c83e49d132c40bb77
runtime_dir="$example_dir/runtime"

for tool in curl shasum unzip; do
    command -v "$tool" >/dev/null 2>&1 || {
        printf 'Required tool not found: %s\n' "$tool" >&2
        exit 1
    }
done

download_dir=$(mktemp -d)
trap 'rm -rf "$download_dir"' EXIT
trap 'exit 1' HUP INT TERM

printf 'Downloading CPython 3.13.11 (WASI SDK 24)...\n'
curl --fail --location --retry 3 --output "$download_dir/$archive" "$url"

(
    cd "$download_dir"
    printf '%s  %s\n' "$expected_sha256" "$archive" | shasum -a 256 -c -
)

# Check the archive before extracting anything into the runtime directory.
unzip -tq "$download_dir/$archive"
mkdir -p "$runtime_dir"
unzip -qo "$download_dir/$archive" -d "$runtime_dir"

printf 'Interpreter: %s/python.wasm\n' "$runtime_dir"
printf 'Standard library: %s/lib/python3.13\n' "$runtime_dir"
