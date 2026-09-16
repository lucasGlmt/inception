//! Lux language server. The protocol layer is intentionally thin: compiler
//! diagnostics come from `lux-compiler`, while attributes and types come
//! from `lux-typeck`.

pub mod analysis;
mod server;
pub mod source_map;

pub use server::Backend;

pub async fn run() {
    let (service, socket) = tower_lsp::LspService::new(Backend::new);
    tower_lsp::Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}
