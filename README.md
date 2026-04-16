<p> 
  <img src="logo.svg" width="160" alt="gabagool logo">
</p>

# gabagool

A WebAssembly interpreter written from scratch. It also contains a time travel debugger.

This project aims to build a fully spec-compliant, performant interpreter whose entire execution state can be serialized, suspended, and restored.
<br>

<details open>
<summary>See demo</summary>
<br>
<img src="demo.gif" width="80%" alt="Game of Life demo"><br>
<em>Each fork snapshots the entire WebAssembly execution state, spawns a brand new process, and resumes exactly where it left off.</em>
<br>
</details>

<details>
<summary>See debugger demo</summary>
<br>
<img src="./gabagool-debug-adapter/dap_demo.gif" width="80%" alt="Time travel debugger demo"><br>
<em>Every step is a full snapshot of gabagool's execution state, so you can jump to any point in history without rerunning the program</em>

<br>
<br>

[![Open in GitHub Codespaces](https://github.com/codespaces/badge.svg)](https://codespaces.new/friendlymatthew/gabagool)

</details>

# Status

`gabagool` is tested against the [WebAssembly spec test suite](https://github.com/WebAssembly/spec/tree/main/test/core). **1,960 tests pass out of 2,049 (96%).** `gabagool` passes on arithmetic, control flow, memory, tables, globals, function references, imports/exports, and exceptions. Remaining tests involve supporting SIMD and garbage collection.

```sh
# run the core test suite
uv run download-core-tests.py
cargo t --features core-tests

# run the component test suite
# you need wasm-tools installed!
cd tests/components && bash fetch_components.sh
cargo t --features component-tests

# run an example wasm program
cargo r -- ./programs/stair_climb.wasm stair_climb 20
```

`gabagool` is not optimized and no serious profiling/benchmarking has been done. That said, the goal is to make it as performant as a pure interpreter can be. The most interesting direction is a translation phase that lowers WASM instructions into a compact intermediate representation, designed for efficient dispatch and serialization.

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

## Time travel debugging

https://awelonblue.wordpress.com/2013/01/24/exponential-decay-of-history-improved/<br>
