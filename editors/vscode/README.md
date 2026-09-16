# Lux for VS Code

This development extension is a thin client for `lux-lsp`. Language semantics remain in the Rust compiler crates.

## Development

From the repository root:

```bash
cargo build -p lux-lsp
cd editors/vscode
npm install
npm run compile
```

Open `editors/vscode` in VS Code and run the **Run Extension** launch configuration, or press `F5`. The extension looks for the server in this order:

1. `lux.server.path`, when configured;
2. `target/debug/lux-lsp` in an open workspace folder or one of its parents;
3. `lux-lsp` on `PATH`.

Set `lux.trace.server` to `messages` or `verbose` to inspect LSP traffic. Server logs use LSP log messages/stderr; stdout remains reserved for the protocol.

This milestone does not bundle platform binaries or publish to the Marketplace.
