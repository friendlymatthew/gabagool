# python

This demo runs the CPython 3.13 interpreter inside `gabagool` via Wasi P1. The full standard library is available, including both pure Python modules and built in C extensions. `/advent25` ctonains Advent of Code 2025 solutions as example scripts

# Usage

```sh
# fetch the interpreter and stdlib
uv run download_python.py

# pass in any python script
cargo r --release howdy.py

# run a advent of code solution
cargo r --release ./advent/day7_1.py
```
