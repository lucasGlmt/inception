use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lux_hir::Capability;
use lux_syntax::ast::{Expression, Item, Literal, SourceFile, Statement};
use lux_syntax::{Span, parse_recovering};
use lux_typeck::{Attribute, ExpectedType, Type};
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, Diagnostic, DiagnosticRelatedInformation,
    DiagnosticSeverity, DocumentSymbol, Documentation, Hover, HoverContents, InsertTextFormat,
    Location, MarkupContent, MarkupKind, ParameterInformation, ParameterLabel, Position, Range,
    SemanticToken, SignatureHelp, SignatureInformation, SymbolKind, TextEdit, Url, WorkspaceEdit,
};

use crate::source_map::SourceMap;

#[derive(Debug, Clone)]
pub struct DocumentSnapshot {
    pub uri: Url,
    pub source: Arc<str>,
    pub map: SourceMap,
    pub ast: SourceFile,
    pub syntax_errors: Vec<lux_syntax::SyntaxError>,
    pub project_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceDatabase {
    roots: Vec<PathBuf>,
    overlays: HashMap<Url, Overlay>,
}

#[derive(Debug, Clone)]
struct Overlay {
    text: Arc<str>,
    version: i32,
}

impl WorkspaceDatabase {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            overlays: HashMap::new(),
        }
    }

    pub fn open(&mut self, uri: Url, text: String, version: i32) {
        self.overlays.insert(
            uri,
            Overlay {
                text: text.into(),
                version,
            },
        );
    }

    pub fn change(&mut self, uri: &Url, text: String, version: i32) {
        if self
            .overlays
            .get(uri)
            .is_none_or(|current| version >= current.version)
        {
            self.open(uri.clone(), text, version);
        }
    }

    pub fn save(&mut self, uri: &Url, text: Option<String>) {
        if let Some(text) = text {
            let version = self.overlays.get(uri).map_or(0, |entry| entry.version);
            self.open(uri.clone(), text, version);
        }
    }

    pub fn close(&mut self, uri: &Url) {
        self.overlays.remove(uri);
    }

    pub fn effective_source(&self, uri: &Url) -> Option<Arc<str>> {
        if let Some(overlay) = self.overlays.get(uri) {
            return Some(overlay.text.clone());
        }
        let path = uri.to_file_path().ok()?;
        fs::read_to_string(path).ok().map(Into::into)
    }

    pub fn snapshot(&self, uri: &Url) -> Option<DocumentSnapshot> {
        let source = self.effective_source(uri)?;
        let (ast, syntax_errors) = parse_recovering(&source);
        let project_root = uri
            .to_file_path()
            .ok()
            .and_then(|path| discover_root(&path))
            .or_else(|| {
                self.roots
                    .iter()
                    .find(|root| root.join("lux.toml").is_file())
                    .cloned()
            });
        Some(DocumentSnapshot {
            uri: uri.clone(),
            map: SourceMap::new(&source),
            source,
            ast,
            syntax_errors,
            project_root,
        })
    }
}

fn discover_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    lux_project::discover_project(start)
        .ok()
        .and_then(|manifest| manifest.parent().map(Path::to_path_buf))
}

#[derive(Debug, Clone)]
pub struct AnalysisSnapshot {
    document: Arc<DocumentSnapshot>,
}

impl AnalysisSnapshot {
    pub fn new(document: DocumentSnapshot) -> Self {
        Self {
            document: Arc::new(document),
        }
    }

    pub fn source(&self) -> &str {
        &self.document.source
    }

    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        let doc = &self.document;
        let mut diagnostics = if doc.syntax_errors.is_empty() {
            match lux_compiler::check(&doc.source, &lux_hir::TargetEnvironment::new()) {
                Ok(_) => Vec::new(),
                Err(errors) => errors
                    .into_iter()
                    .map(|error| {
                        let related_information = error.secondary_span.map(|span| {
                            vec![DiagnosticRelatedInformation {
                                location: Location::new(
                                    doc.uri.clone(),
                                    doc.map.range(&doc.source, span),
                                ),
                                message: error
                                    .help
                                    .clone()
                                    .unwrap_or_else(|| "related location".into()),
                            }]
                        });
                        Diagnostic {
                            range: doc.map.range(&doc.source, error.span),
                            severity: Some(DiagnosticSeverity::ERROR),
                            code: Some(tower_lsp::lsp_types::NumberOrString::String(format!(
                                "lux::{:?}",
                                error.stage
                            ))),
                            code_description: None,
                            source: Some("lux".into()),
                            message: error.message,
                            related_information,
                            tags: None,
                            data: error.help.map(serde_json::Value::String),
                        }
                    })
                    .collect(),
            }
        } else {
            let expected_at_eof = self.expected_type(doc.source.len()).map(ExpectedType::ty);
            let mut seen_spans = HashSet::new();
            doc.syntax_errors
                .iter()
                .filter(|error| seen_spans.insert(error.span))
                .map(|error| Diagnostic {
                    range: doc.map.range(&doc.source, error.span),
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: Some(tower_lsp::lsp_types::NumberOrString::String(
                        "lux::Syntax".into(),
                    )),
                    code_description: None,
                    source: Some("lux".into()),
                    message: if error.span.start == doc.source.len() {
                        expected_at_eof
                            .map(|ty| format!("expected `{ty}`"))
                            .unwrap_or_else(|| error.message.clone())
                    } else {
                        error.message.clone()
                    },
                    related_information: None,
                    tags: None,
                    data: error.help.clone().map(serde_json::Value::String),
                })
                .collect()
        };
        diagnostics.extend(self.lints());
        diagnostics
    }

    fn lints(&self) -> Vec<Diagnostic> {
        let mut result = Vec::new();
        for item in &self.document.ast.items {
            let Item::Scene(scene) = item else { continue };
            let mut uses = HashSet::new();
            for statement in &scene.body.statements {
                visit_statement_expressions(statement, &mut |expression| {
                    if let Expression::Identifier(identifier) = expression {
                        uses.insert(identifier.name.clone());
                    }
                });
                if let Statement::Transition(transition) = statement
                    && matches!(
                        transition.duration,
                        Expression::Literal(Literal::Duration(0), _)
                    )
                {
                    result.push(self.lint(
                        transition.duration.span(),
                        DiagnosticSeverity::HINT,
                        "zero-duration-transition",
                        "this transition is equivalent to an immediate assignment",
                    ));
                }
            }
            for statement in &scene.body.statements {
                if let Statement::Let(binding) = statement
                    && !binding.name.name.is_empty()
                    && !uses.contains(&binding.name.name)
                {
                    result.push(self.lint(
                        binding.name.span,
                        DiagnosticSeverity::WARNING,
                        "unused-local",
                        format!("unused variable `{}`", binding.name.name),
                    ));
                }
            }
        }
        result
    }

    fn lint(
        &self,
        span: Span,
        severity: DiagnosticSeverity,
        code: &str,
        message: impl Into<String>,
    ) -> Diagnostic {
        Diagnostic {
            range: self.document.map.range(&self.document.source, span),
            severity: Some(severity),
            code: Some(tower_lsp::lsp_types::NumberOrString::String(code.into())),
            code_description: None,
            source: Some("lux-lint".into()),
            message: message.into(),
            related_information: None,
            tags: None,
            data: None,
        }
    }

    pub fn complete(&self, position: Position) -> Vec<CompletionItem> {
        let offset = self.document.map.offset(&self.document.source, position);
        let prefix = identifier_prefix(&self.document.source, offset);

        if let Some(segments) = import_path_segments(&self.document.source, offset) {
            return import_completions(&segments, prefix);
        }

        if let Some(receiver) = member_receiver(&self.document.source, offset) {
            if let Some(module) = self
                .imported_std_modules()
                .into_iter()
                .find(|module| module.short_name == receiver)
            {
                return std_member_completions(module, prefix);
            }
            return self
                .role_members(receiver)
                .into_iter()
                .filter(|item| matches_prefix(&item.label, prefix))
                .collect();
        }

        let expected = self.expected_type(offset);
        let mut items = Vec::new();
        for local in self.visible_locals(offset) {
            let priority = if local.ty == expected.map(ExpectedType::ty) {
                "0"
            } else {
                "2"
            };
            if matches_prefix(&local.name, prefix) {
                items.push(CompletionItem {
                    label: local.name,
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: local.ty.map(|ty| ty.to_string()),
                    sort_text: Some(format!("{priority}-local")),
                    ..CompletionItem::default()
                });
            }
        }
        for role in self.roles() {
            if matches_prefix(&role.name, prefix) {
                items.push(CompletionItem {
                    label: role.name,
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some(role.detail),
                    sort_text: Some("2-role".into()),
                    ..CompletionItem::default()
                });
            }
        }
        if let Some(expected) = expected {
            items.extend(
                literals(expected.ty())
                    .into_iter()
                    .filter(|item| matches_prefix(&item.label, prefix)),
            );
        }
        if is_top_level(&self.document.source, offset) {
            for keyword in ["scene", "rig"] {
                if matches_prefix(keyword, prefix) {
                    items.push(keyword_item(keyword, "4-keyword"));
                }
            }
        } else if statement_start(&self.document.source, offset) {
            for keyword in ["let", "wait"] {
                if matches_prefix(keyword, prefix) {
                    items.push(keyword_item(keyword, "4-keyword"));
                }
            }
        }
        items
    }

    pub fn expected_type(&self, offset: usize) -> Option<ExpectedType> {
        if let Some(expected) = self.expected_call_argument_type(offset) {
            return Some(expected);
        }
        let before = &self.document.source[..offset.min(self.document.source.len())];
        let statement = before
            .rsplit([';', '{', '}'])
            .next()
            .unwrap_or(before)
            .trim();
        if let Some(over) = statement.rfind("over")
            && statement[..over].contains("->")
            && statement[over + 4..].trim().is_empty()
        {
            return Some(ExpectedType::transition_duration());
        }
        if statement.starts_with("wait ") {
            return Some(ExpectedType::wait_value());
        }
        if let Some((lhs, rhs)) = statement.rsplit_once("->")
            && !rhs.contains("over")
        {
            return expected_attribute_from_lhs(lhs);
        }
        if let Some((lhs, _)) = statement.rsplit_once('=') {
            if let Some(expected) = expected_attribute_from_lhs(lhs) {
                return Some(expected);
            }
            if let Some(annotation) = lhs.rsplit_once(':').map(|(_, ty)| ty.trim()) {
                return Type::from_name(annotation).map(ExpectedType::exact);
            }
        }
        None
    }

    /// The expected type of the argument position the cursor sits in,
    /// inside a qualified stdlib call (`Color.rgb(255, $0`) — delegates
    /// entirely to `lux_typeck::ExpectedType::for_call_argument`, which
    /// itself defers to `lux_stdlib`'s registry; this crate never repeats
    /// the signature data.
    fn expected_call_argument_type(&self, offset: usize) -> Option<ExpectedType> {
        let (open_paren, active_param) = enclosing_call_paren(&self.document.source, offset)?;
        let (qualifier, name) = call_name_before(&self.document.source, open_paren)?;
        let module = self
            .imported_std_modules()
            .into_iter()
            .find(|module| module.short_name == qualifier)?;
        ExpectedType::for_call_argument(module.path, name, active_param)
    }

    /// Signature help for the call the cursor is currently inside,
    /// listing every overload of `qualifier.name` (see
    /// `lux_stdlib::candidates`) so an overloaded function like
    /// `Math.abs` shows both its `Int` and `Float` signatures.
    pub fn signature_help(&self, position: Position) -> Option<SignatureHelp> {
        let offset = self.document.map.offset(&self.document.source, position);
        let (open_paren, active_param) = enclosing_call_paren(&self.document.source, offset)?;
        let (qualifier, name) = call_name_before(&self.document.source, open_paren)?;
        let module = self
            .imported_std_modules()
            .into_iter()
            .find(|module| module.short_name == qualifier)?;
        let candidates = lux_stdlib::candidates(module.path, name);
        if candidates.is_empty() {
            return None;
        }

        // Prefer the first overload whose arity can still fit the
        // parameter the cursor is on; falls back to the first overload
        // when every candidate is already too short (mid-typing).
        let active_signature = candidates
            .iter()
            .position(|sig| active_param < sig.params.len())
            .unwrap_or(0) as u32;

        let signatures = candidates
            .iter()
            .map(|sig| SignatureInformation {
                label: signature_label(sig),
                documentation: Some(Documentation::String(sig.doc.into())),
                parameters: Some(
                    sig.params
                        .iter()
                        .map(|param| ParameterInformation {
                            label: ParameterLabel::Simple(format!(
                                "{}: {}",
                                param.name,
                                param_type_name(param.ty)
                            )),
                            documentation: None,
                        })
                        .collect(),
                ),
                active_parameter: None,
            })
            .collect();

        Some(SignatureHelp {
            signatures,
            active_signature: Some(active_signature),
            active_parameter: Some(active_param as u32),
        })
    }

    fn roles(&self) -> Vec<RoleInfo> {
        self.document
            .ast
            .items
            .iter()
            .filter_map(|item| match item {
                Item::RigContract(contract) => Some(&contract.roles),
                _ => None,
            })
            .flatten()
            .map(|role| {
                let capabilities: Vec<_> = role
                    .capabilities
                    .iter()
                    .filter_map(|cap| capability(&cap.name))
                    .collect();
                let names = role
                    .capabilities
                    .iter()
                    .map(|cap| cap.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" + ");
                RoleInfo {
                    name: role.name.name.clone(),
                    span: role.name.span,
                    capabilities,
                    detail: format!("Group<{names}>"),
                }
            })
            .collect()
    }

    /// Every `std.*` module this document actually imports, resolved
    /// against `lux_stdlib`'s registry directly — the LSP never hardcodes
    /// its own copy of the module/function list. An import of an unknown
    /// module is simply absent here (its own diagnostic comes from
    /// `diagnostics()`, via `lux_compiler::check`).
    fn imported_std_modules(&self) -> Vec<&'static lux_stdlib::StdModule> {
        self.document
            .ast
            .items
            .iter()
            .filter_map(|item| {
                let Item::Import(import) = item else {
                    return None;
                };
                let segments: Vec<&str> = import.path.iter().map(|id| id.name.as_str()).collect();
                (segments.first() == Some(&"std"))
                    .then(|| lux_stdlib::find_module(&segments))
                    .flatten()
            })
            .collect()
    }

    fn role_members(&self, receiver: &str) -> Vec<CompletionItem> {
        let Some(role) = self.roles().into_iter().find(|role| role.name == receiver) else {
            return Vec::new();
        };
        Attribute::ALL
            .iter()
            .copied()
            .filter(|attribute| role.capabilities.contains(&attribute.required_capability()))
            .map(|attribute| CompletionItem {
                label: attribute.name().into(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(attribute.value_type().to_string()),
                sort_text: Some("1-member".into()),
                ..CompletionItem::default()
            })
            .collect()
    }

    fn visible_locals(&self, offset: usize) -> Vec<LocalInfo> {
        let mut result = Vec::new();
        for item in &self.document.ast.items {
            let Item::Scene(scene) = item else { continue };
            if !(scene.span.start <= offset && offset <= scene.span.end) {
                continue;
            }
            for statement in &scene.body.statements {
                let Statement::Let(binding) = statement else {
                    continue;
                };
                if binding.name.span.start >= offset || binding.name.name.is_empty() {
                    continue;
                }
                let ty = binding
                    .type_annotation
                    .as_ref()
                    .and_then(|annotation| Type::from_name(&annotation.name))
                    .or_else(|| expression_type(&binding.value));
                result.push(LocalInfo {
                    name: binding.name.name.clone(),
                    span: binding.name.span,
                    ty,
                });
            }
        }
        result
    }

    pub fn hover(&self, position: Position) -> Option<Hover> {
        let offset = self.document.map.offset(&self.document.source, position);
        let word = word_at(&self.document.source, offset)?;
        let mut value = None;
        if let Some(module) =
            qualifier_before(&self.document.source, word.1).and_then(|qualifier| {
                self.imported_std_modules()
                    .into_iter()
                    .find(|module| module.short_name == qualifier)
            })
        {
            if let Some(sig) = module.functions.iter().find(|sig| sig.name == word.0) {
                value = Some(format!(
                    "```lux\n{}\n```\n\n{}",
                    signature_label(sig),
                    sig.doc
                ));
            }
        } else if let Some(module) = self
            .imported_std_modules()
            .into_iter()
            .find(|module| module.short_name == word.0)
        {
            value = Some(format!(
                "```lux\nmodule {}\n```\n\n{}",
                module.short_name, module.doc
            ));
        } else if let Some(role) = self.roles().into_iter().find(|role| role.name == word.0) {
            value = Some(format!("```lux\nrole {}\n{}\n```", role.name, role.detail));
        } else if let Some(local) = self
            .visible_locals(offset + word.0.len())
            .into_iter()
            .find(|local| local.name == word.0)
        {
            value = Some(format!(
                "```lux\nlet {}: {}\n```",
                local.name,
                local.ty.map_or("unknown".into(), |ty| ty.to_string())
            ));
        } else if let Some(attribute) = Attribute::from_name(word.0) {
            let range = if attribute == Attribute::Intensity {
                "\nrange: 0%..100%"
            } else {
                ""
            };
            value = Some(format!(
                "```lux\n{}\n```{}\n\nprovided by capability `{:?}`",
                attribute.value_type(),
                range,
                attribute.required_capability()
            ));
        } else if let Some(ty) = Type::from_name(word.0) {
            value = Some(format!("```lux\ntype {ty}\n```"));
        } else if self.scene_names().contains(&word.0) {
            value = Some(format!("```lux\nscene {}\n```", word.0));
        }
        value.map(|value| Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(
                self.document
                    .map
                    .range(&self.document.source, Span::new(word.1, word.2)),
            ),
        })
    }

    pub fn definition(&self, position: Position) -> Option<Location> {
        let offset = self.document.map.offset(&self.document.source, position);
        let (word, _, _) = word_at(&self.document.source, offset)?;
        let span = self
            .roles()
            .into_iter()
            .find(|role| role.name == word)
            .map(|role| role.span)
            .or_else(|| self.local_definition(word, offset))
            .or_else(|| self.scene_definition(word));
        span.map(|span| {
            Location::new(
                self.document.uri.clone(),
                self.document.map.range(&self.document.source, span),
            )
        })
    }

    pub fn references(&self, position: Position, include_declaration: bool) -> Vec<Location> {
        let offset = self.document.map.offset(&self.document.source, position);
        let Some((word, _, _)) = word_at(&self.document.source, offset) else {
            return Vec::new();
        };
        let Some(definition) = self.symbol_definition(word, offset) else {
            return Vec::new();
        };
        self.symbol_occurrences(word, definition)
            .into_iter()
            .filter(|span| include_declaration || *span != definition)
            .map(|span| {
                Location::new(
                    self.document.uri.clone(),
                    self.document.map.range(&self.document.source, span),
                )
            })
            .collect()
    }

    pub fn prepare_rename(&self, position: Position) -> Option<Range> {
        let offset = self.document.map.offset(&self.document.source, position);
        let (word, start, end) = word_at(&self.document.source, offset)?;
        self.symbol_definition(word, offset)?;
        Some(
            self.document
                .map
                .range(&self.document.source, Span::new(start, end)),
        )
    }

    pub fn rename(&self, position: Position, new_name: String) -> Option<WorkspaceEdit> {
        if !valid_identifier(&new_name) {
            return None;
        }
        let offset = self.document.map.offset(&self.document.source, position);
        let (word, _, _) = word_at(&self.document.source, offset)?;
        let definition = self.symbol_definition(word, offset)?;
        let edits = self
            .symbol_occurrences(word, definition)
            .into_iter()
            .map(|span| {
                TextEdit::new(
                    self.document.map.range(&self.document.source, span),
                    new_name.clone(),
                )
            })
            .collect();
        Some(WorkspaceEdit {
            changes: Some(HashMap::from([(self.document.uri.clone(), edits)])),
            document_changes: None,
            change_annotations: None,
        })
    }

    fn symbol_definition(&self, word: &str, offset: usize) -> Option<Span> {
        self.roles()
            .into_iter()
            .find(|role| role.name == word)
            .map(|role| role.span)
            .or_else(|| self.local_definition(word, offset))
            .or_else(|| self.scene_definition(word))
    }

    fn local_definition(&self, word: &str, offset: usize) -> Option<Span> {
        self.visible_locals(offset + word.len())
            .into_iter()
            .rev()
            .find(|local| local.name == word)
            .map(|local| local.span)
    }

    fn scene_definition(&self, word: &str) -> Option<Span> {
        self.document.ast.items.iter().find_map(|item| match item {
            Item::Scene(scene) if scene.name.name == word => Some(scene.name.span),
            _ => None,
        })
    }

    fn symbol_occurrences(&self, word: &str, definition: Span) -> Vec<Span> {
        let role = self.roles().iter().any(|role| role.span == definition);
        let scene = self.scene_definition(word) == Some(definition);
        let local_scene = (!role && !scene)
            .then(|| {
                self.document.ast.items.iter().find_map(|item| {
                    match item {
                Item::Scene(scene) if scene.body.statements.iter().any(|statement| {
                    matches!(statement, Statement::Let(binding) if binding.name.span == definition)
                }) => Some(scene.span),
                _ => None,
            }
                })
            })
            .flatten();
        let mut spans = vec![definition];
        for item in &self.document.ast.items {
            if let Item::Scene(scene_decl) = item {
                if let Some(owner) = local_scene
                    && scene_decl.span != owner
                {
                    continue;
                }
                for statement in &scene_decl.body.statements {
                    match statement {
                        Statement::Assign(assign) if role && assign.target.name == word => {
                            spans.push(assign.target.span)
                        }
                        Statement::Transition(transition)
                            if role && transition.target.name == word =>
                        {
                            spans.push(transition.target.span)
                        }
                        _ => {}
                    }
                    if !role && !scene {
                        visit_statement_expressions(statement, &mut |expression| {
                            if let Expression::Identifier(identifier) = expression
                                && identifier.name == word
                                && identifier.span.start >= definition.start
                            {
                                spans.push(identifier.span);
                            }
                        });
                    }
                }
            }
        }
        spans.sort_by_key(|span| span.start);
        spans.dedup();
        spans
    }

    #[allow(deprecated)]
    pub fn document_symbols(&self) -> Vec<DocumentSymbol> {
        let mut symbols = Vec::new();
        for item in &self.document.ast.items {
            match item {
                Item::RigContract(contract) => {
                    let children = contract
                        .roles
                        .iter()
                        .map(|role| DocumentSymbol {
                            name: role.name.name.clone(),
                            detail: Some(format!(
                                "Group<{}>",
                                role.capabilities
                                    .iter()
                                    .map(|cap| cap.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" + ")
                            )),
                            kind: SymbolKind::VARIABLE,
                            tags: None,
                            deprecated: None,
                            range: self.document.map.range(&self.document.source, role.span),
                            selection_range: self
                                .document
                                .map
                                .range(&self.document.source, role.name.span),
                            children: None,
                        })
                        .collect();
                    symbols.push(DocumentSymbol {
                        name: contract.name.name.clone(),
                        detail: Some("rig contract".into()),
                        kind: SymbolKind::NAMESPACE,
                        tags: None,
                        deprecated: None,
                        range: self
                            .document
                            .map
                            .range(&self.document.source, contract.span),
                        selection_range: self
                            .document
                            .map
                            .range(&self.document.source, contract.name.span),
                        children: Some(children),
                    });
                }
                Item::Scene(scene) => symbols.push(DocumentSymbol {
                    name: scene.name.name.clone(),
                    detail: Some("scene".into()),
                    kind: SymbolKind::FUNCTION,
                    tags: None,
                    deprecated: None,
                    range: self.document.map.range(&self.document.source, scene.span),
                    selection_range: self
                        .document
                        .map
                        .range(&self.document.source, scene.name.span),
                    children: None,
                }),
                Item::Import(import) => symbols.push(DocumentSymbol {
                    name: import
                        .path
                        .iter()
                        .map(|segment| segment.name.as_str())
                        .collect::<Vec<_>>()
                        .join("."),
                    detail: Some("import".into()),
                    kind: SymbolKind::MODULE,
                    tags: None,
                    deprecated: None,
                    range: self.document.map.range(&self.document.source, import.span),
                    selection_range: self.document.map.range(&self.document.source, import.span),
                    children: None,
                }),
            }
        }
        symbols
    }

    /// Semantic token legend indices: variable, function, parameter, type,
    /// property. Tokens are sorted and delta encoded as required by LSP.
    pub fn semantic_tokens(&self) -> Vec<SemanticToken> {
        let mut absolute: Vec<(Position, u32, u32)> = Vec::new();
        let mut push = |span: Span, token_type: u32| {
            let start = self
                .document
                .map
                .position(&self.document.source, span.start);
            let end = self.document.map.position(&self.document.source, span.end);
            if start.line == end.line && end.character > start.character {
                absolute.push((start, end.character - start.character, token_type));
            }
        };
        for item in &self.document.ast.items {
            match item {
                Item::RigContract(contract) => {
                    push(contract.name.span, 3);
                    for role in &contract.roles {
                        push(role.name.span, 0);
                        for capability in &role.capabilities {
                            push(capability.span, 3);
                        }
                    }
                }
                Item::Scene(scene) => {
                    push(scene.name.span, 1);
                    for statement in &scene.body.statements {
                        if let Statement::Let(binding) = statement {
                            push(binding.name.span, 0);
                            if let Some(annotation) = &binding.type_annotation {
                                push(annotation.span, 3);
                            }
                        }
                        match statement {
                            Statement::Assign(assign) => {
                                push(assign.target.span, 0);
                                push(assign.attribute.span, 4);
                            }
                            Statement::Transition(transition) => {
                                push(transition.target.span, 0);
                                push(transition.attribute.span, 4);
                            }
                            _ => {}
                        }
                        visit_statement_expressions(statement, &mut |expression| {
                            if let Expression::Identifier(identifier) = expression {
                                push(identifier.span, 0);
                            }
                            if let Expression::Call(call) = expression {
                                if let Some(qualifier) = &call.callee.qualifier {
                                    push(qualifier.span, 5);
                                }
                                push(call.callee.name.span, 1);
                            }
                        });
                    }
                }
                Item::Import(import) => {
                    for segment in &import.path {
                        push(segment.span, 5);
                    }
                }
            }
        }
        absolute.sort_by_key(|(position, _, _)| (position.line, position.character));
        absolute.dedup();
        let mut previous_line = 0;
        let mut previous_character = 0;
        absolute
            .into_iter()
            .map(|(position, length, token_type)| {
                let delta_line = position.line - previous_line;
                let delta_start = if delta_line == 0 {
                    position.character - previous_character
                } else {
                    position.character
                };
                previous_line = position.line;
                previous_character = position.character;
                SemanticToken {
                    delta_line,
                    delta_start,
                    length,
                    token_type,
                    token_modifiers_bitset: 0,
                }
            })
            .collect()
    }

    fn scene_names(&self) -> Vec<&str> {
        self.document
            .ast
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Scene(scene) => Some(scene.name.name.as_str()),
                _ => None,
            })
            .collect()
    }
}

#[derive(Debug)]
struct RoleInfo {
    name: String,
    span: Span,
    capabilities: Vec<Capability>,
    detail: String,
}
#[derive(Debug)]
struct LocalInfo {
    name: String,
    span: Span,
    ty: Option<Type>,
}

fn capability(name: &str) -> Option<Capability> {
    match name {
        "Intensity" => Some(Capability::Intensity),
        "Color" => Some(Capability::Color),
        _ => None,
    }
}

fn expression_type(expression: &Expression) -> Option<Type> {
    match expression {
        Expression::Literal(literal, _) => Some(lux_typeck::literal_type(*literal)),
        Expression::Grouped(inner, _) => expression_type(inner),
        _ => None,
    }
}

fn literals(ty: Type) -> Vec<CompletionItem> {
    let values: &[&str] = match ty {
        Type::Intensity => &["0%", "25%", "50%", "75%", "100%"],
        Type::Duration => &["100ms", "250ms", "500ms", "1s", "2s"],
        Type::Color => &["red", "green", "blue", "white", "black", "#ffffff"],
        Type::Bool => &["true", "false"],
        _ => &[],
    };
    values
        .iter()
        .map(|value| CompletionItem {
            label: (*value).into(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(ty.to_string()),
            sort_text: Some("3-literal".into()),
            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
            documentation: Some(Documentation::String(format!("Lux `{ty}` literal"))),
            ..CompletionItem::default()
        })
        .collect()
}

fn keyword_item(keyword: &str, sort: &str) -> CompletionItem {
    CompletionItem {
        label: keyword.into(),
        kind: Some(CompletionItemKind::KEYWORD),
        sort_text: Some(sort.into()),
        ..CompletionItem::default()
    }
}

fn matches_prefix(label: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || label
            .to_ascii_lowercase()
            .starts_with(&prefix.to_ascii_lowercase())
}

fn identifier_prefix(source: &str, offset: usize) -> &str {
    let before = &source[..offset.min(source.len())];
    let start = before
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_alphanumeric() && *ch != '_')
        .map_or(0, |(i, ch)| i + ch.len_utf8());
    &before[start..]
}

fn member_receiver(source: &str, offset: usize) -> Option<&str> {
    let before = &source[..offset.min(source.len())];
    let prefix = identifier_prefix(source, offset);
    let head = &before[..before.len() - prefix.len()];
    let head = head.strip_suffix('.')?;
    let start = head
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_alphanumeric() && *ch != '_')
        .map_or(0, |(i, ch)| i + ch.len_utf8());
    let receiver = &head[start..];
    (!receiver.is_empty()).then_some(receiver)
}

/// If `word_start` is immediately preceded by `<ident>.` (ignoring
/// nothing in between — no whitespace is tolerated, matching Lux's
/// dotted-call syntax), returns that identifier. Used by `hover` to
/// recognize `Math` in `Math.sin` when the cursor is on `sin`.
fn qualifier_before(source: &str, word_start: usize) -> Option<&str> {
    let before = &source[..word_start.min(source.len())];
    let before = before.strip_suffix('.')?;
    let start = before
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_alphanumeric() && *ch != '_')
        .map_or(0, |(i, ch)| i + ch.len_utf8());
    let receiver = &before[start..];
    (!receiver.is_empty()).then_some(receiver)
}

/// Detects an in-progress `import` path at `offset`, returning the
/// segments already terminated by a `.` (not including whatever partial
/// segment is still being typed — that's `identifier_prefix`'s job at the
/// call site). `import std.$0` returns `["std"]`; a bare `import $0`
/// returns `[]`; anything that isn't inside an `import` statement's path
/// returns `None`.
fn import_path_segments(source: &str, offset: usize) -> Option<Vec<String>> {
    let before = &source[..offset.min(source.len())];
    let statement = before.rsplit(';').next().unwrap_or(before);
    let rest = statement.trim_start().strip_prefix("import")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let mut segments = Vec::new();
    let mut current = String::new();
    for ch in rest.trim_start().chars() {
        if ch == '.' {
            segments.push(std::mem::take(&mut current));
        } else if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
        } else {
            return None;
        }
    }
    Some(segments)
}

fn import_completions(segments: &[String], prefix: &str) -> Vec<CompletionItem> {
    let module_item = |label: &str, detail: &str| CompletionItem {
        label: label.into(),
        kind: Some(CompletionItemKind::MODULE),
        detail: Some(detail.into()),
        sort_text: Some("0-module".into()),
        ..CompletionItem::default()
    };
    match segments {
        [] => ["std"]
            .into_iter()
            .filter(|name| matches_prefix(name, prefix))
            .map(|name| module_item(name, "standard library"))
            .collect(),
        [root] if root == "std" => lux_stdlib::STD_MODULES
            .iter()
            .filter(|module| matches_prefix(module.short_name, prefix))
            .map(|module| module_item(module.short_name, module.doc))
            .collect(),
        _ => Vec::new(),
    }
}

/// Member completions for an imported `std` module, deduplicating
/// overloads (e.g. `Math.abs`'s `Int`/`Float` signatures) down to one
/// item per function name, with every overload's signature listed in
/// `detail`.
fn std_member_completions(
    module: &'static lux_stdlib::StdModule,
    prefix: &str,
) -> Vec<CompletionItem> {
    let mut seen = HashSet::new();
    module
        .functions
        .iter()
        .filter(|sig| seen.insert(sig.name))
        .filter(|sig| matches_prefix(sig.name, prefix))
        .map(|sig| {
            let overloads = lux_stdlib::candidates(module.path, sig.name);
            let detail = overloads
                .iter()
                .map(signature_label)
                .collect::<Vec<_>>()
                .join(" | ");
            CompletionItem {
                label: sig.name.into(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some(detail),
                documentation: Some(Documentation::String(sig.doc.into())),
                insert_text: Some(format!("{}($0)", sig.name)),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                sort_text: Some("1-member".into()),
                ..CompletionItem::default()
            }
        })
        .collect()
}

fn signature_label(sig: &lux_stdlib::Signature) -> String {
    let params = sig
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param_type_name(param.ty)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{}({params}) -> {}",
        sig.name,
        param_type_name(sig.return_ty)
    )
}

fn param_type_name(ty: lux_stdlib::ParamType) -> &'static str {
    match ty {
        lux_stdlib::ParamType::Int => "Int",
        lux_stdlib::ParamType::Float => "Float",
        lux_stdlib::ParamType::Angle => "Angle",
        lux_stdlib::ParamType::Intensity => "Intensity",
        lux_stdlib::ParamType::Color => "Color",
        lux_stdlib::ParamType::Unsupported => "?",
    }
}

/// Scans backward from `offset` for the nearest unmatched `(` at depth 0,
/// returning its byte offset together with how many top-level commas lie
/// between it and `offset` — that count is exactly the index of the
/// argument the cursor is currently in. Returns `None` once a `;`/`{`/`}`
/// is hit at depth 0, meaning `offset` isn't inside a call's argument
/// list at all.
fn enclosing_call_paren(source: &str, offset: usize) -> Option<(usize, usize)> {
    let before = &source[..offset.min(source.len())];
    let mut depth = 0i32;
    let mut comma_count = 0usize;
    for (i, ch) in before.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                if depth == 0 {
                    return Some((i, comma_count));
                }
                depth -= 1;
            }
            ',' if depth == 0 => comma_count += 1,
            ';' | '{' | '}' if depth == 0 => return None,
            _ => {}
        }
    }
    None
}

/// Given the byte offset of a call's opening `(`, returns the
/// `(qualifier, name)` immediately preceding it — e.g. for
/// `...Color.rgb(...`, `("Color", "rgb")`. `None` if there's no qualified
/// name there (an unqualified or malformed call).
fn call_name_before(source: &str, open_paren: usize) -> Option<(&str, &str)> {
    let before = source[..open_paren].trim_end();
    let name_start = before
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_alphanumeric() && *ch != '_')
        .map_or(0, |(i, ch)| i + ch.len_utf8());
    let name = &before[name_start..];
    if name.is_empty() {
        return None;
    }
    let qualifier = qualifier_before(before, name_start)?;
    Some((qualifier, name))
}

fn word_at(source: &str, offset: usize) -> Option<(&str, usize, usize)> {
    let offset = offset.min(source.len());
    let mut start = offset;
    while start > 0
        && (source.as_bytes()[start - 1].is_ascii_alphanumeric()
            || source.as_bytes()[start - 1] == b'_')
    {
        start -= 1;
    }
    let mut end = offset;
    while end < source.len()
        && (source.as_bytes()[end].is_ascii_alphanumeric() || source.as_bytes()[end] == b'_')
    {
        end += 1;
    }
    (start < end).then(|| (&source[start..end], start, end))
}

fn expected_attribute_from_lhs(lhs: &str) -> Option<ExpectedType> {
    ExpectedType::for_attribute(lhs.trim().rsplit_once('.')?.1.trim())
}

fn valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_top_level(source: &str, offset: usize) -> bool {
    let mut depth = 0i32;
    for byte in source[..offset.min(source.len())].bytes() {
        match byte {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
    }
    depth <= 0
}

fn statement_start(source: &str, offset: usize) -> bool {
    source[..offset.min(source.len())]
        .rsplit([';', '{', '}'])
        .next()
        .is_none_or(|text| {
            text.trim()
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        })
}

fn visit_statement_expressions(statement: &Statement, visitor: &mut impl FnMut(&Expression)) {
    match statement {
        Statement::Let(binding) => visit_expression(&binding.value, visitor),
        Statement::Wait(wait) => visit_expression(&wait.value, visitor),
        Statement::Expression(expression) => visit_expression(&expression.expr, visitor),
        Statement::Assign(assign) => visit_expression(&assign.value, visitor),
        Statement::Transition(transition) => {
            visit_expression(&transition.value, visitor);
            visit_expression(&transition.duration, visitor);
        }
    }
}

fn visit_expression(expression: &Expression, visitor: &mut impl FnMut(&Expression)) {
    visitor(expression);
    match expression {
        Expression::Unary(unary) => visit_expression(&unary.operand, visitor),
        Expression::Binary(binary) => {
            visit_expression(&binary.lhs, visitor);
            visit_expression(&binary.rhs, visitor);
        }
        Expression::Call(call) => {
            for arg in &call.args {
                visit_expression(arg, visitor);
            }
        }
        Expression::Grouped(inner, _) => visit_expression(inner, visitor),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn snapshot(marked: &str) -> (AnalysisSnapshot, Position) {
        let offset = marked.find("$0").unwrap();
        let source = marked.replace("$0", "");
        let uri = Url::parse("file:///test.lux").unwrap();
        let map = SourceMap::new(&source);
        let position = map.position(&source, offset);
        let (ast, syntax_errors) = parse_recovering(&source);
        (
            AnalysisSnapshot::new(DocumentSnapshot {
                uri,
                map,
                source: source.into(),
                ast,
                syntax_errors,
                project_root: None,
            }),
            position,
        )
    }

    #[test]
    fn capability_completion_is_filtered() {
        let (analysis, position) = snapshot(
            "rig contract Demo { role Washes: Group<Color + Intensity>; } scene main { Washes.$0 }",
        );
        let labels: Vec<_> = analysis
            .complete(position)
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert_eq!(labels, ["intensity", "color"]);
        assert!(!labels.contains(&"pan".into()));
    }

    #[test]
    fn duration_expected_after_over_and_typed_local_ranks_first() {
        let (analysis, position) = snapshot(
            "rig contract Demo { role Washes: Group<Intensity>; } scene main { let fade: Duration = 2s; Washes.intensity -> 100% over $0 }",
        );
        assert_eq!(
            analysis.expected_type(analysis.document.map.offset(analysis.source(), position)),
            Some(ExpectedType::exact(Type::Duration))
        );
        let items = analysis.complete(position);
        let fade = items.iter().find(|item| item.label == "fade").unwrap();
        assert_eq!(fade.sort_text.as_deref(), Some("0-local"));
        assert!(items.iter().any(|item| item.label == "500ms"));
    }

    #[test]
    fn incomplete_sources_never_panic() {
        for source in [
            "W",
            "Washes.",
            "Washes.intensity =",
            "Washes.intensity ->",
            "Washes.intensity -> 100% over",
            "scene",
            "scene main {",
        ] {
            let marked = format!("{source}$0");
            let (analysis, position) = snapshot(&marked);
            let _ = analysis.diagnostics();
            let _ = analysis.complete(position);
            let _ = analysis.hover(position);
        }
    }

    #[test]
    fn incomplete_transition_reports_one_contextual_eof_error() {
        let (analysis, _) = snapshot(
            "rig contract Demo { role Washes: Group<Intensity>; } scene main { Washes.intensity -> 100% over$0",
        );
        let errors: Vec<_> = analysis
            .diagnostics()
            .into_iter()
            .filter(|diagnostic| diagnostic.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert_eq!(
            errors.len(),
            1,
            "coalesce parser cascades at the same EOF position"
        );
        assert_eq!(errors[0].message, "expected `Duration`");
    }

    #[test]
    fn overlay_wins_then_close_falls_back_to_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("show.lux");
        fs::write(&path, "scene disk { wait 1s; }").unwrap();
        let uri = Url::from_file_path(&path).unwrap();
        let mut database = WorkspaceDatabase::default();
        database.open(uri.clone(), "scene memory { wait red; }".into(), 1);
        let overlay = AnalysisSnapshot::new(database.snapshot(&uri).unwrap());
        assert!(
            overlay
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Duration"))
        );
        database.close(&uri);
        let disk = AnalysisSnapshot::new(database.snapshot(&uri).unwrap());
        assert!(disk.diagnostics().is_empty());
    }

    #[test]
    fn compiler_type_diagnostic_keeps_precise_message() {
        let (analysis, _) = snapshot(
            "rig contract Demo { role Washes: Group<Color + Intensity>; } scene main { Washes.intensity -> red over 2s; $0}",
        );
        let diagnostics = analysis.diagnostics();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == "expected `Intensity`, found `Color`")
        );
    }

    #[test]
    fn hover_and_definition_understand_roles_and_attributes() {
        let source = "rig contract Demo { role Washes: Group<Color + Intensity>; } scene main { Washes.intensity = 50%; }";
        let role_use = source.rfind("Washes").unwrap() + 2;
        let attribute = source.rfind("intensity").unwrap() + 2;
        let marked = format!("{}$0{}", &source[..role_use], &source[role_use..]);
        let (analysis, position) = snapshot(&marked);
        let definition = analysis.definition(position).unwrap();
        assert_eq!(definition.range.start, Position::new(0, 25));
        let hover_position = analysis.document.map.position(analysis.source(), attribute);
        let hover = analysis.hover(hover_position).unwrap();
        let HoverContents::Markup(contents) = hover.contents else {
            panic!("expected markup")
        };
        assert!(contents.value.contains("Intensity"));
        assert!(contents.value.contains("0%..100%"));
    }

    #[test]
    fn rename_is_semantic_and_does_not_touch_comments_or_homonymous_locals() {
        let source = "rig contract Demo { role Washes: Group<Intensity>; } // Washes\nscene main { Washes.intensity = 50%; let Washes = 1s; wait Washes; }";
        let role_use = source.find("Washes.intensity").unwrap() + 2;
        let marked = format!("{}$0{}", &source[..role_use], &source[role_use..]);
        let (analysis, position) = snapshot(&marked);
        let edit = analysis.rename(position, "FrontWashes".into()).unwrap();
        let edits = edit.changes.unwrap().into_values().next().unwrap();
        assert_eq!(
            edits.len(),
            2,
            "only role declaration and target use are renamed"
        );
    }

    #[test]
    fn import_path_completion_proposes_std_modules() {
        let (analysis, position) = snapshot("import std.$0");
        let labels: Vec<_> = analysis
            .complete(position)
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert_eq!(labels, ["Math", "Color"]);
    }

    #[test]
    fn bare_import_proposes_std_root() {
        let (analysis, position) = snapshot("import $0");
        let labels: Vec<_> = analysis
            .complete(position)
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert_eq!(labels, ["std"]);
    }

    #[test]
    fn std_module_member_completion_lists_its_functions() {
        let (analysis, position) = snapshot("import std.Math;\nscene main { let x = Math.$0 }");
        let labels: Vec<_> = analysis
            .complete(position)
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert_eq!(labels, ["sin", "cos", "abs", "min", "max", "clamp", "lerp"]);
    }

    #[test]
    fn unimported_module_falls_back_to_role_members() {
        // `Math` isn't imported here, so `.` completion must not silently
        // show stdlib members for it, and must not panic either.
        let (analysis, position) = snapshot("scene main { let x = Math.$0 }");
        assert!(analysis.complete(position).is_empty());
    }

    #[test]
    fn hover_on_qualified_stdlib_call_shows_its_signature() {
        let source = "import std.Math;\nscene main { let x = Math.sin(90deg); }";
        let sin_pos = source.find("sin").unwrap() + 1;
        let marked = format!("{}$0{}", &source[..sin_pos], &source[sin_pos..]);
        let (analysis, position) = snapshot(&marked);
        let hover = analysis.hover(position).unwrap();
        let HoverContents::Markup(contents) = hover.contents else {
            panic!("expected markup")
        };
        assert!(contents.value.contains("sin(angle: Angle) -> Float"));
    }

    #[test]
    fn hover_on_module_qualifier_shows_module_doc() {
        let source = "import std.Color;\nscene main { let x = Color.rgb(1, 2, 3); }";
        let pos = source.find("Color.rgb").unwrap() + 1;
        let marked = format!("{}$0{}", &source[..pos], &source[pos..]);
        let (analysis, position) = snapshot(&marked);
        let hover = analysis.hover(position).unwrap();
        let HoverContents::Markup(contents) = hover.contents else {
            panic!("expected markup")
        };
        assert!(contents.value.contains("module Color"));
    }

    #[test]
    fn signature_help_reports_active_parameter_and_signature() {
        let source = "import std.Color;\nscene main { let x = Color.rgb(255, ";
        let marked = format!("{source}$0");
        let (analysis, position) = snapshot(&marked);
        let help = analysis.signature_help(position).unwrap();
        assert_eq!(help.active_parameter, Some(1));
        assert_eq!(help.signatures.len(), 1);
        assert!(help.signatures[0].label.starts_with("rgb(r: Int"));
    }

    #[test]
    fn signature_help_lists_every_overload() {
        let source = "import std.Math;\nscene main { let x = Math.abs(";
        let marked = format!("{source}$0");
        let (analysis, position) = snapshot(&marked);
        let help = analysis.signature_help(position).unwrap();
        assert_eq!(help.signatures.len(), 2);
    }

    #[test]
    fn expected_type_understands_stdlib_call_arguments() {
        let source = "import std.Math;\nscene main { let x = Math.sin(";
        let marked = format!("{source}$0");
        let (analysis, position) = snapshot(&marked);
        let offset = analysis.document.map.offset(analysis.source(), position);
        assert_eq!(
            analysis.expected_type(offset),
            Some(ExpectedType::exact(Type::Angle))
        );
    }

    #[test]
    fn local_references_do_not_cross_scene_scope() {
        let source = "scene one { let fade: Duration = 1s; wait fade; } scene two { let fade: Duration = 2s; wait fade; }";
        let first_use = source.find("wait fade").unwrap() + 6;
        let marked = format!("{}$0{}", &source[..first_use], &source[first_use..]);
        let (analysis, position) = snapshot(&marked);
        assert_eq!(analysis.references(position, true).len(), 2);
    }
}
