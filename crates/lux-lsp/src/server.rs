use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::analysis::{AnalysisSnapshot, WorkspaceDatabase};

pub struct Backend {
    client: Client,
    database: Arc<RwLock<WorkspaceDatabase>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            database: Arc::new(RwLock::new(WorkspaceDatabase::default())),
        }
    }

    async fn analysis(&self, uri: &Url) -> Option<AnalysisSnapshot> {
        self.database
            .read()
            .await
            .snapshot(uri)
            .map(AnalysisSnapshot::new)
    }

    async fn publish(&self, uri: Url) {
        let diagnostics = self
            .analysis(&uri)
            .await
            .map_or_else(Vec::new, |a| a.diagnostics());
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let roots: Vec<PathBuf> = params
            .workspace_folders
            .unwrap_or_default()
            .into_iter()
            .filter_map(|folder| folder.uri.to_file_path().ok())
            .collect();
        *self.database.write().await = WorkspaceDatabase::new(roots);
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".into(), " ".into()]),
                    resolve_provider: Some(false),
                    ..CompletionOptions::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into(), ",".into()]),
                    retrigger_characters: None,
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                document_symbol_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: vec![
                                    SemanticTokenType::VARIABLE,
                                    SemanticTokenType::FUNCTION,
                                    SemanticTokenType::PARAMETER,
                                    SemanticTokenType::TYPE,
                                    SemanticTokenType::PROPERTY,
                                    SemanticTokenType::NAMESPACE,
                                ],
                                token_modifiers: vec![],
                            },
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                        },
                    ),
                ),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "lux-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Lux language server ready")
            .await;
    }
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        self.database
            .write()
            .await
            .open(doc.uri.clone(), doc.text, doc.version);
        self.publish(doc.uri).await;
    }
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };
        let doc = params.text_document;
        self.database
            .write()
            .await
            .change(&doc.uri, change.text, doc.version);
        self.publish(doc.uri).await;
    }
    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.database
            .write()
            .await
            .save(&params.text_document.uri, params.text);
        self.publish(params.text_document.uri).await;
    }
    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.database.write().await.close(&uri);
        self.publish(uri).await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let p = params.text_document_position;
        Ok(self
            .analysis(&p.text_document.uri)
            .await
            .map(|a| CompletionResponse::Array(a.complete(p.position))))
    }
    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let p = params.text_document_position_params;
        Ok(self
            .analysis(&p.text_document.uri)
            .await
            .and_then(|a| a.hover(p.position)))
    }
    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let p = params.text_document_position_params;
        Ok(self
            .analysis(&p.text_document.uri)
            .await
            .and_then(|a| a.signature_help(p.position)))
    }
    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let p = params.text_document_position_params;
        Ok(self
            .analysis(&p.text_document.uri)
            .await
            .and_then(|a| a.definition(p.position))
            .map(GotoDefinitionResponse::Scalar))
    }
    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let p = params.text_document_position;
        Ok(self
            .analysis(&p.text_document.uri)
            .await
            .map(|a| a.references(p.position, params.context.include_declaration)))
    }
    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        Ok(self
            .analysis(&params.text_document.uri)
            .await
            .and_then(|a| a.prepare_rename(params.position))
            .map(PrepareRenameResponse::Range))
    }
    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let p = params.text_document_position;
        let Some(analysis) = self.analysis(&p.text_document.uri).await else {
            return Ok(None);
        };
        analysis
            .rename(p.position, params.new_name)
            .map(Some)
            .ok_or_else(|| Error::invalid_params("symbol cannot be renamed or new name is invalid"))
    }
    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        Ok(self
            .analysis(&params.text_document.uri)
            .await
            .map(|a| DocumentSymbolResponse::Nested(a.document_symbols())))
    }
    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        Ok(self.analysis(&params.text_document.uri).await.map(|a| {
            SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: a.semantic_tokens(),
            })
        }))
    }
}
