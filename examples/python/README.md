# CPython on gabagool

This example runs cpython compiled to wasm inside gabagool using WASI preview 1

Python scripts can import modules, read inputs, and print results

# Usage
```sh
# https://github.com/brettcannon/cpython-wasi-build/releases/tag/v3.13.11
sh examples/python/download.sh

# cd into examples/python
cargo r --release
```

The `aoc/` directory is mounted read-only at `/aoc`. Run day one from
`examples/python`:

```sh
cargo r --release -- /aoc/day1_1.py
```
