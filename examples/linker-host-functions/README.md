# Linker host functions

This example shows how to use `gabagool::Linker` to satisfy a wasm function import with a Rust host function.

The embedded wasm module imports one function:

- `host.add_one`: takes an `i32` and returns an `i32`

The wasm `run` export passes `66` to `host.add_one`, and the Rust host function returns `67`.

# Usage

```sh
cd ./examples/linker-host-functions
cargo r
```

Expected output:

```text
wasm returned: 67
```
