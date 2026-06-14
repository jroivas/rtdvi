//! Position-based requests: goto-*, references, rename, range formatting,
//! and hover, plus the response-shape parsing helpers.

use std::time::Duration;
use lsp_types::request::Request;
use lsp_types::Url;
use serde_json::Value;

use super::Client;

impl Client {
    /// Request a definition jump. Returns `(uri, line, character)` for the
    /// first location in the response, or `None` if the server didn't
    /// return a useful answer.
    pub fn goto_definition(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Vec<(String, u32, u32)> {
        if !self.capabilities.definition {
            return Vec::new();
        }
        self.request_all_locations(
            lsp_types::request::GotoDefinition::METHOD,
            uri,
            line,
            character,
        )
    }

    /// Common shape for definition/declaration/implementation/typeDefinition.
    /// All four return `Location | Location[] | LocationLink[]`; we collect
    /// every location so the caller can show a picker when multiple exist.
    fn request_all_locations(
        &mut self,
        method: &'static str,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Vec<(String, u32, u32)> {
        let Ok(parsed) = Url::parse(uri) else {
            return Vec::new();
        };
        let params = lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: parsed },
            position: lsp_types::Position { line, character },
        };
        let Some(result) = self.request_sync(
            method,
            serde_json::to_value(params).unwrap(),
            Duration::from_millis(1500),
        ) else {
            return Vec::new();
        };
        all_locations(result)
    }

    pub fn goto_declaration(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Vec<(String, u32, u32)> {
        if !self.capabilities.declaration {
            return Vec::new();
        }
        self.request_all_locations(
            lsp_types::request::GotoDeclaration::METHOD,
            uri,
            line,
            character,
        )
    }

    pub fn goto_implementation(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Vec<(String, u32, u32)> {
        if !self.capabilities.implementation {
            return Vec::new();
        }
        self.request_all_locations(
            lsp_types::request::GotoImplementation::METHOD,
            uri,
            line,
            character,
        )
    }

    pub fn goto_type_definition(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Vec<(String, u32, u32)> {
        if !self.capabilities.type_definition {
            return Vec::new();
        }
        self.request_all_locations(
            lsp_types::request::GotoTypeDefinition::METHOD,
            uri,
            line,
            character,
        )
    }

    /// `textDocument/references`. Returns every location the server
    /// reports (across files). `include_declaration` controls whether the
    /// declaration site itself is part of the result.
    pub fn references(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Vec<(String, u32, u32)> {
        if !self.capabilities.references {
            return Vec::new();
        }
        let Ok(parsed) = Url::parse(uri) else {
            return Vec::new();
        };
        let params = lsp_types::ReferenceParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: parsed },
                position: lsp_types::Position { line, character },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext { include_declaration },
        };
        let Some(result) = self.request_sync(
            lsp_types::request::References::METHOD,
            serde_json::to_value(params).unwrap(),
            Duration::from_millis(2000),
        ) else {
            return Vec::new();
        };
        let locs: Vec<lsp_types::Location> =
            serde_json::from_value(result).unwrap_or_default();
        locs.into_iter()
            .map(|l| (l.uri.to_string(), l.range.start.line, l.range.start.character))
            .collect()
    }

    /// `textDocument/rename`. Returns the resulting `WorkspaceEdit` for
    /// the caller to apply, or `None` if the server refuses or times out.
    pub fn rename(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Option<lsp_types::WorkspaceEdit> {
        if !self.capabilities.rename {
            return None;
        }
        let Ok(parsed) = Url::parse(uri) else {
            return None;
        };
        let params = lsp_types::RenameParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: parsed },
                position: lsp_types::Position { line, character },
            },
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        };
        let result = self.request_sync(
            lsp_types::request::Rename::METHOD,
            serde_json::to_value(params).unwrap(),
            Duration::from_millis(3000),
        )?;
        serde_json::from_value(result).ok()
    }

    /// `textDocument/rangeFormatting` for the inclusive line range
    /// `[start_line, end_line]`. Returns the server's edits, or `None`.
    pub fn range_formatting(
        &mut self,
        uri: &str,
        start_line: u32,
        end_line: u32,
        tab_size: u32,
        insert_spaces: bool,
    ) -> Option<Vec<lsp_types::TextEdit>> {
        if !self.capabilities.range_formatting {
            return None;
        }
        let Ok(parsed) = Url::parse(uri) else {
            return None;
        };
        // Format whole lines: column 0 of the first line to column 0 of the
        // line after the last (an end-exclusive line range).
        let params = lsp_types::DocumentRangeFormattingParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: parsed },
            range: lsp_types::Range {
                start: lsp_types::Position { line: start_line, character: 0 },
                end: lsp_types::Position { line: end_line + 1, character: 0 },
            },
            options: lsp_types::FormattingOptions {
                tab_size,
                insert_spaces,
                ..Default::default()
            },
            work_done_progress_params: Default::default(),
        };
        let result = self.request_sync(
            lsp_types::request::RangeFormatting::METHOD,
            serde_json::to_value(params).unwrap(),
            Duration::from_millis(3000),
        )?;
        serde_json::from_value(result).ok()
    }

    /// Request hover text. Returns a plain-text excerpt suitable for the
    /// statusline.
    pub fn hover(&mut self, uri: &str, line: u32, character: u32) -> Option<String> {
        if !self.capabilities.hover {
            return None;
        }
        let Ok(parsed) = Url::parse(uri) else { return None };
        let params = lsp_types::HoverParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: parsed },
                position: lsp_types::Position { line, character },
            },
            work_done_progress_params: Default::default(),
        };
        let result = self.request_sync(
            lsp_types::request::HoverRequest::METHOD,
            serde_json::to_value(params).unwrap(),
            Duration::from_millis(1500),
        )?;
        let hover: lsp_types::Hover = serde_json::from_value(result).ok()?;
        Some(hover_text(&hover))
    }
}

/// Extract `(uri, line, character)` from any of the three shapes that
/// definition/declaration/implementation/typeDefinition can return.
fn all_locations(result: Value) -> Vec<(String, u32, u32)> {
    if let Ok(loc) = serde_json::from_value::<lsp_types::Location>(result.clone()) {
        return vec![(loc.uri.to_string(), loc.range.start.line, loc.range.start.character)];
    }
    if let Ok(locs) = serde_json::from_value::<Vec<lsp_types::Location>>(result.clone()) {
        return locs
            .into_iter()
            .map(|l| (l.uri.to_string(), l.range.start.line, l.range.start.character))
            .collect();
    }
    if let Ok(links) = serde_json::from_value::<Vec<lsp_types::LocationLink>>(result) {
        return links
            .into_iter()
            .map(|l| {
                (
                    l.target_uri.to_string(),
                    l.target_selection_range.start.line,
                    l.target_selection_range.start.character,
                )
            })
            .collect();
    }
    Vec::new()
}

fn hover_text(hover: &lsp_types::Hover) -> String {
    use lsp_types::{HoverContents, MarkedString};
    match &hover.contents {
        HoverContents::Scalar(MarkedString::String(s)) => s.clone(),
        HoverContents::Scalar(MarkedString::LanguageString(l)) => l.value.clone(),
        HoverContents::Array(arr) => arr
            .iter()
            .map(|s| match s {
                MarkedString::String(s) => s.clone(),
                MarkedString::LanguageString(l) => l.value.clone(),
            })
            .collect::<Vec<_>>()
            .join(" "),
        HoverContents::Markup(m) => m.value.clone(),
    }
}
