//! Width-independent display content retained after an MCP call completes.
//!
//! The wire result can contain multi-megabyte image, audio, or resource bodies. Validate each
//! block once with the MCP model, then retain only what history rendering actually displays.

use crate::exec_cell::TOOL_CALL_MAX_LINES;
use crate::terminal_images::ImagePreview;
use crate::text_formatting::format_and_truncate_tool_result;
use codex_protocol::mcp::CallToolResult;
use rmcp::model::ContentBlock;
use rmcp::model::ResourceContents;
use serde::Deserialize;
use std::borrow::Cow;
use tracing::error;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum McpResultKind {
    Standard,
    NodeRepl,
}

#[derive(Debug)]
pub(super) struct McpToolResult {
    pub(super) content: Vec<McpContentBlock>,
    pub(super) is_error: bool,
    pub(super) image: Option<ImagePreview>,
}

#[derive(Debug)]
pub(super) struct McpContentBlock {
    display: McpContentDisplay,
    /// Code mode uses a top-level `text` field even on malformed or non-text blocks.
    original_text: Option<String>,
}

#[derive(Debug)]
enum McpContentDisplay {
    Text(String),
    Summary(Cow<'static, str>),
    Json(String),
}

impl McpToolResult {
    /// Consumes a wire result, dropping bodies represented only by media or resource summaries.
    ///
    /// Canonical MCP deserialization preserves full text and the exact JSON fallback for malformed
    /// or unknown blocks. Every block is projected, but image decoding stops at the first fully
    /// valid image.
    pub(super) fn new(result: CallToolResult, kind: McpResultKind) -> Self {
        let mut preview = None;
        let content = result
            .content
            .into_iter()
            .map(|block| {
                // Deserialize by reference so malformed blocks remain available for the exact
                // JSON fallback. Successful blocks no longer retain their wire representation.
                let parsed = ContentBlock::deserialize(&block);
                let original_text = match (&parsed, kind) {
                    (Ok(ContentBlock::Text(_)), _) | (_, McpResultKind::Standard) => None,
                    (_, McpResultKind::NodeRepl) => block
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                };
                let display = match parsed {
                    Ok(ContentBlock::Text(text)) => McpContentDisplay::Text(text.text),
                    Ok(ContentBlock::Image(image)) => {
                        // Retain at most one decoded thumbnail per result.
                        if preview.is_none() {
                            match ImagePreview::from_base64(&image.data) {
                                Ok(image) => preview = Some(image),
                                Err(error) => error!(%error, "Image preview decoding failed"),
                            }
                        }
                        McpContentDisplay::Summary("<image content>".into())
                    }
                    Ok(ContentBlock::Audio(_)) => {
                        McpContentDisplay::Summary("<audio content>".into())
                    }
                    Ok(ContentBlock::Resource(resource)) => {
                        let summary = match resource.resource {
                            ResourceContents::TextResourceContents { uri, .. }
                            | ResourceContents::BlobResourceContents { uri, .. } => {
                                format!("embedded resource: {uri}").into()
                            }
                            _ => "<unknown embedded resource>".into(),
                        };
                        McpContentDisplay::Summary(summary)
                    }
                    Ok(ContentBlock::ResourceLink(link)) => {
                        McpContentDisplay::Summary(format!("link: {}", link.uri).into())
                    }
                    Ok(_) | Err(_) => McpContentDisplay::Json(block.to_string()),
                };
                McpContentBlock {
                    display,
                    original_text,
                }
            })
            .collect();

        Self {
            content,
            is_error: result.is_error.unwrap_or(false),
            image: preview,
        }
    }
}

impl McpContentBlock {
    /// Returns the untruncated top-level text used by node_repl and cua_repl's compact and
    /// transcript views.
    ///
    /// Valid text blocks reuse their display storage. Only node_repl and cua_repl retain this extra
    /// field on malformed or non-text blocks, where it takes precedence over the usual display
    /// content.
    pub(super) fn text(&self) -> Option<&str> {
        match &self.display {
            McpContentDisplay::Text(text) => Some(text),
            McpContentDisplay::Summary(_) | McpContentDisplay::Json(_) => {
                self.original_text.as_deref()
            }
        }
    }

    /// Applies width-dependent formatting to full text or fallback JSON without reparsing the MCP
    /// block. Media and resource summaries retain their existing untruncated display form.
    pub(super) fn render(&self, width: usize) -> String {
        match &self.display {
            McpContentDisplay::Text(text) | McpContentDisplay::Json(text) => {
                format_and_truncate_tool_result(text, TOOL_CALL_MAX_LINES, width)
            }
            McpContentDisplay::Summary(summary) => summary.to_string(),
        }
    }
}
