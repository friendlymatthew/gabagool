<p>
  <img src="logo.svg" width="160" alt="gabagool logo">
</p>

# gabagool

A WebAssembly interpreter written from scratch.

This project aims to build a **serious**, fully spec-compliant, performant WASM interpreter whose entire execution state can be serialized, suspended, and restored.
<br>

<details open>
<summary>See demo</summary>
<br>
<img src="demo.gif" width="80%" alt="Game of Life demo"><br>
<em>Each fork snapshots the entire WebAssembly execution state, spawns a brand new process, and resumes exactly where it left off.</em>
<br>
</details>

# Status

As a pure interpreter, it is safe to assume `gabagool` is spec compliant. `gabagool` is tested against the [WebAssembly spec test suite](https://github.com/WebAssembly/spec/tree/main/test/core). **1,960 tests pass out of 2,049 (96%).** `gabagool` passes on arithmetic, control flow, memory, tables, globals, function references, imports/exports, and exceptions. Remaining tests involve supporting SIMD and garbage collection.

`gabagool` is not focused on performance optimization. No serious profiling/benchmarking has been done (yet!). That said, the goal is to make `gabagool` performant while preserving its snapshot friendly execution model. `gabagool` already lowers WASM instructions into a compact, serializable intermediate representation. The most interesting direction now is an experimental JIT compiler based on copy and patch compilation.

**Current work** focuses on how gabagool interacts with its host environment. This includes a minimal, virtualized WASI Preview 1 implementation whose state can be snapshotted and restored alongside the interpreter.

# Usage

```sh
# run the core test suite
uv run download-core-tests.py
cargo t --features core-tests --test core_tests

# run the component test suite
# you need wasm-tools installed!
cd tests/components && bash fetch_components.sh
cargo t --features component-tests

# run an example wasm program
cargo r -- ./test-programs/stair_climb.wasm stair_climb 20
```

# Reading

https://webassembly.github.io/spec/core/<br>
https://github.com/bytecodealliance/wasmtime/issues/3017<br>
https://github.com/bytecodealliance/wasmtime/issues/4002<br>

## Wasm Component Model

https://www.infoq.com/podcasts/web-assembly-component-model/<br>
https://blog.sunfishcode.online/what-is-a-wasm-component/<br>
https://www.fermyon.com/blog/webassembly-component-model<br>
https://github.com/WebAssembly/component-model/blob/main/design/mvp/Explainer.md<br>
https://github.com/WebAssembly/component-model/blob/main/design/mvp/Binary.md<br>
https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md<br>

## Copy and patch compilation

https://fredrikbk.com/publications/copy-and-patch.pdf<br>
https://www.youtube.com/watch?v=HxSHIpEQRjs<br>
