# CPython on gabagool

This example runs cpython compiled to wasm inside gabagool using WASI preview 1. Python scripts can import modules, read inputs, and print results

It includes my Advent of Code 2025 solutions to demonstrate the interpreter running real Python programs

# Usage
```sh
# https://github.com/brettcannon/cpython-wasi-build/releases/tag/v3.13.11
sh examples/python/download.sh

# cd into examples/python
cargo r --release

cargo r --release -- /aoc/day1_1.py

# benchmark aoc problems (both python and gabagool)
cargo bench --bench aoc

# profile aoc problems 
cargo b --release
samply record ./target/release/python aoc/day7_1.py
```
