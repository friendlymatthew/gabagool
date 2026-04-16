# gabagool-debug-adapter

A DAP server that enables time travel debugging for WebAssembly programs.

Currently, the debugger steps through `.wat` source files. Supporting DWARF debug symbols to step through the original source code is a future (and lofty) goal.

<img src="dap_demo.gif" width="80%" alt="Time travel debugger demo"><br>

# Try it in one click

[![Open in GitHub Codespaces](https://github.com/codespaces/badge.svg)](https://codespaces.new/friendlymatthew/gabagool)

Click the badge, wait for the container to build, then open any `.wat` file under `test-programs/` and press `F5`. The devcontainer builds the adapter and installs the VS Code extension for you.

# Local install for VSCode

```sh
# build the adapter and copy the binary into the extension dir
./gabagool-debug-adapter/local-install.sh

# symlink the extension into vscode
ln -sfn "$(pwd)/gabagool-debug-adapter" ~/.vscode/extensions/gabagool-debug

# reload vscode (cmd + shift + p -> "Developer: reload window")
# open any .wat file and press F5, then pick a program from the dropdown
```

# Reading

https://microsoft.github.io/debug-adapter-protocol/specification.html<br>
