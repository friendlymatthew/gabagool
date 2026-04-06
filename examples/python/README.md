# python

This demo runs the CPython 3.13 interpreter compiled to wasm inside `gabagool`. It executes Python scripts by loading `python.wasm` with WASI P1 support.

# Usage

```sh
# fetch the interpreter and stdlib
uv run download_python.py

# pass in any python script
cargo r --release hello.py
```
