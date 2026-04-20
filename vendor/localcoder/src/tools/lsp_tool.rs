/*!
 * LspTool — S19
 *
 * Provides code navigation through Language Server Protocol (LSP) backends.
 */

use crate::services::lsp::LspServerManager;
use crate::services::lsp::manager::WorkspaceSymbolBatch;
use crate::services::lsp::types::{CallHierarchyDirection, LspOperation, file_path_from_uri};
use crate::tools::Tool;
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub struct LspTool {
    manager: LspServerManager,
}

impl LspTool {
    pub fn new(cwd: &Path) -> Result<Self> {
        Ok(Self {
            manager: LspServerManager::new(cwd)?,
        })
    }

    fn unavailable_message(&self) -> String {
        format!(
            "LSP is unavailable. Configure .localcoder/settings.json under lsp.servers or install a built-in server.\n{}",
            self.manager.render_status()
        )
    }
}

impl Tool for LspTool {
    fn name(&self) -> &str {
        "Lsp"
    }

    fn description(&self) -> &str {
        "Code intelligence via language servers. Supports go_to_definition, find_references, hover, document_symbols, workspace_symbols, and call_hierarchy."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": [
                        "go_to_definition",
                        "find_references",
                        "hover",
                        "document_symbols",
                        "workspace_symbols",
                        "call_hierarchy"
                    ],
                    "description": "LSP operation to perform"
                },
                "file": {
                    "type": "string",
                    "description": "Absolute or relative file path for file-based operations"
                },
                "line": {
                    "type": "integer",
                    "description": "1-based line number for position-based operations"
                },
                "character": {
                    "type": "integer",
                    "description": "1-based character offset for position-based operations"
                },
                "query": {
                    "type": "string",
                    "description": "Workspace symbol query for workspace_symbols"
                },
                "direction": {
                    "type": "string",
                    "enum": ["incoming", "outgoing"],
                    "description": "Call hierarchy direction. Defaults to incoming."
                },
                "include_declaration": {
                    "type": "boolean",
                    "description": "Whether reference search should include the declaration. Defaults to true."
                }
            },
            "required": ["operation"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        if !self.manager.has_servers() {
            return Ok(self.unavailable_message());
        }

        let request = ToolInput::from_value(input)?;
        match request.operation {
            LspOperation::GoToDefinition => {
                let file = request.require_file()?;
                let position = request.require_position()?;
                let params = text_document_position_params(&file, position)?;
                match self
                    .manager
                    .request_for_file(&file, "textDocument/definition", params)
                    .await?
                {
                    Some((server_name, value)) => {
                        let locations = parse_location_targets(&value)?;
                        Ok(format_locations(
                            "go_to_definition",
                            &server_name,
                            locations.as_slice(),
                            self.manager.workspace_root(),
                        ))
                    }
                    None => Ok(no_server_for_file(&file)),
                }
            }
            LspOperation::FindReferences => {
                let file = request.require_file()?;
                let position = request.require_position()?;
                let params = text_document_position_params(&file, position)?.tap_mut(|value| {
                    value["context"] = json!({
                        "includeDeclaration": request.include_declaration.unwrap_or(true)
                    });
                });
                match self
                    .manager
                    .request_for_file(&file, "textDocument/references", params)
                    .await?
                {
                    Some((server_name, value)) => {
                        let locations = parse_locations(&value)?;
                        Ok(format_locations(
                            "find_references",
                            &server_name,
                            locations.as_slice(),
                            self.manager.workspace_root(),
                        ))
                    }
                    None => Ok(no_server_for_file(&file)),
                }
            }
            LspOperation::Hover => {
                let file = request.require_file()?;
                let position = request.require_position()?;
                let params = text_document_position_params(&file, position)?;
                match self
                    .manager
                    .request_for_file(&file, "textDocument/hover", params)
                    .await?
                {
                    Some((server_name, value)) => Ok(format_hover(&server_name, &value)?),
                    None => Ok(no_server_for_file(&file)),
                }
            }
            LspOperation::DocumentSymbols => {
                let file = request.require_file()?;
                let params = json!({
                    "textDocument": {"uri": file_uri_for_input(&file)?}
                });
                match self
                    .manager
                    .request_for_file(&file, "textDocument/documentSymbol", params)
                    .await?
                {
                    Some((server_name, value)) => Ok(format_document_symbols(
                        &server_name,
                        &value,
                        self.manager.workspace_root(),
                    )?),
                    None => Ok(no_server_for_file(&file)),
                }
            }
            LspOperation::WorkspaceSymbols => {
                let query = request
                    .query
                    .as_deref()
                    .map(str::trim)
                    .filter(|query| !query.is_empty())
                    .ok_or_else(|| anyhow!("workspace_symbols requires a non-empty 'query'"))?;
                let batch = self.manager.request_workspace_symbols(query).await?;
                Ok(format_workspace_symbols(
                    query,
                    batch,
                    self.manager.workspace_root(),
                )?)
            }
            LspOperation::CallHierarchy => {
                let file = request.require_file()?;
                let position = request.require_position()?;
                let direction = request
                    .direction
                    .unwrap_or(CallHierarchyDirection::Incoming);
                let params = text_document_position_params(&file, position)?;
                match self
                    .manager
                    .request_for_file(&file, "textDocument/prepareCallHierarchy", params)
                    .await?
                {
                    Some((server_name, value)) => {
                        let items = parse_call_hierarchy_items(&value)?;
                        if items.is_empty() {
                            return Ok(format!(
                                "Operation: call_hierarchy\nServer: {}\nDirection: {}\nNo call hierarchy item found at the requested position.",
                                server_name,
                                direction.as_str()
                            ));
                        }

                        let method = match direction {
                            CallHierarchyDirection::Incoming => "callHierarchy/incomingCalls",
                            CallHierarchyDirection::Outgoing => "callHierarchy/outgoingCalls",
                        };
                        let Some((server_name, calls)) = self
                            .manager
                            .request_for_file(&file, method, json!({"item": items[0]}))
                            .await?
                        else {
                            return Ok(no_server_for_file(&file));
                        };
                        Ok(format_call_hierarchy(
                            &server_name,
                            direction,
                            &calls,
                            self.manager.workspace_root(),
                        )?)
                    }
                    None => Ok(no_server_for_file(&file)),
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ToolInput {
    operation: String,
    file: Option<String>,
    line: Option<u32>,
    character: Option<u32>,
    query: Option<String>,
    direction: Option<String>,
    include_declaration: Option<bool>,
}

impl ToolInput {
    fn from_value(input: Value) -> Result<ParsedInput> {
        let raw: Self = serde_json::from_value(input).context("invalid Lsp input")?;
        let operation = LspOperation::parse(&raw.operation)
            .ok_or_else(|| anyhow!("unknown LSP operation: {}", raw.operation))?;
        let direction = match raw.direction {
            Some(direction) => Some(
                CallHierarchyDirection::parse(&direction)
                    .ok_or_else(|| anyhow!("invalid call hierarchy direction: {}", direction))?,
            ),
            None => None,
        };
        Ok(ParsedInput {
            operation,
            file: raw.file,
            line: raw.line,
            character: raw.character,
            query: raw.query,
            direction,
            include_declaration: raw.include_declaration,
        })
    }
}

#[derive(Debug)]
struct ParsedInput {
    operation: LspOperation,
    file: Option<String>,
    line: Option<u32>,
    character: Option<u32>,
    query: Option<String>,
    direction: Option<CallHierarchyDirection>,
    include_declaration: Option<bool>,
}

impl ParsedInput {
    fn require_file(&self) -> Result<PathBuf> {
        let file = self
            .file
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "operation '{}' requires a non-empty 'file'",
                    self.operation.as_str()
                )
            })?;
        Ok(PathBuf::from(file))
    }

    fn require_position(&self) -> Result<(u32, u32)> {
        let line = self.line.filter(|value| *value > 0).ok_or_else(|| {
            anyhow!(
                "operation '{}' requires 'line' > 0",
                self.operation.as_str()
            )
        })?;
        let character = self.character.filter(|value| *value > 0).ok_or_else(|| {
            anyhow!(
                "operation '{}' requires 'character' > 0",
                self.operation.as_str()
            )
        })?;
        Ok((line, character))
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Position {
    line: u32,
    character: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct Range {
    start: Position,
}

#[derive(Debug, Clone, Deserialize)]
struct Location {
    uri: String,
    range: Range,
}

#[derive(Debug, Clone, Deserialize)]
struct LocationLink {
    #[serde(rename = "targetUri")]
    target_uri: String,
    #[serde(rename = "targetSelectionRange")]
    target_selection_range: Option<Range>,
    #[serde(rename = "targetRange")]
    target_range: Range,
}

#[derive(Debug, Clone, Deserialize)]
struct SymbolInformation {
    name: String,
    kind: u32,
    location: Location,
    #[serde(rename = "containerName")]
    container_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceSymbol {
    name: String,
    kind: u32,
    location: WorkspaceSymbolLocation,
    #[serde(rename = "containerName")]
    container_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum WorkspaceSymbolLocation {
    Location(Location),
    UriOnly { uri: String },
}

#[derive(Debug, Clone, Deserialize)]
struct DocumentSymbol {
    name: String,
    kind: u32,
    range: Range,
    #[serde(default)]
    children: Vec<DocumentSymbol>,
}

#[derive(Debug, Clone, Deserialize)]
struct MarkupContent {
    kind: Option<String>,
    value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum HoverContents {
    Scalar(String),
    Markup(MarkupContent),
    Array(Vec<HoverContents>),
    LanguageString(BTreeMap<String, String>),
}

#[derive(Debug, Clone, Deserialize)]
struct HoverResult {
    contents: HoverContents,
}

#[derive(Debug, Clone, Deserialize)]
struct CallHierarchyItem {
    name: String,
    kind: u32,
    uri: String,
    range: Range,
}

#[derive(Debug, Clone, Deserialize)]
struct IncomingCall {
    from: CallHierarchyItem,
    #[serde(rename = "fromRanges")]
    from_ranges: Vec<Range>,
}

#[derive(Debug, Clone, Deserialize)]
struct OutgoingCall {
    to: CallHierarchyItem,
    #[serde(rename = "fromRanges")]
    from_ranges: Vec<Range>,
}

fn text_document_position_params(file: &Path, (line, character): (u32, u32)) -> Result<Value> {
    Ok(json!({
        "textDocument": {"uri": file_uri_for_input(file)?},
        "position": {
            "line": line.saturating_sub(1),
            "character": character.saturating_sub(1),
        }
    }))
}

fn file_uri_for_input(file: &Path) -> Result<String> {
    let candidate = if file.is_absolute() {
        file.to_path_buf()
    } else {
        std::env::current_dir()?.join(file)
    };
    let absolute = candidate.canonicalize().unwrap_or(candidate);
    crate::services::lsp::types::file_uri(&absolute)
}

fn no_server_for_file(file: &Path) -> String {
    format!(
        "No LSP server is configured for '{}'. Add a matching server under lsp.servers in .localcoder/settings.json.",
        file.display()
    )
}

fn parse_locations(value: &Value) -> Result<Vec<Location>> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    serde_json::from_value(value.clone()).context("failed to parse LSP locations")
}

fn parse_location_targets(value: &Value) -> Result<Vec<Location>> {
    if value.is_null() {
        return Ok(Vec::new());
    }

    if let Ok(locations) = serde_json::from_value::<Vec<Location>>(value.clone()) {
        return Ok(locations);
    }
    if let Ok(single) = serde_json::from_value::<Location>(value.clone()) {
        return Ok(vec![single]);
    }
    if let Ok(links) = serde_json::from_value::<Vec<LocationLink>>(value.clone()) {
        return Ok(links.into_iter().map(Location::from).collect());
    }
    if let Ok(single) = serde_json::from_value::<LocationLink>(value.clone()) {
        return Ok(vec![Location::from(single)]);
    }

    Err(anyhow!("failed to parse LSP definition target"))
}

fn parse_call_hierarchy_items(value: &Value) -> Result<Vec<Value>> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    if let Some(array) = value.as_array() {
        return Ok(array.clone());
    }
    Err(anyhow!("failed to parse call hierarchy preparation result"))
}

fn format_locations(
    operation: &str,
    server_name: &str,
    locations: &[Location],
    workspace_root: &Path,
) -> String {
    let mut out = format!("Operation: {}\nServer: {}", operation, server_name);
    if locations.is_empty() {
        out.push_str("\nNo results found.");
        return out;
    }

    out.push_str(&format!("\nResults: {}", locations.len()));
    for (index, location) in locations.iter().enumerate() {
        out.push_str(&format!(
            "\n{}. {}",
            index + 1,
            format_location_line(location, workspace_root)
        ));
    }
    out
}

fn format_hover(server_name: &str, value: &Value) -> Result<String> {
    if value.is_null() {
        return Ok(format!(
            "Operation: hover\nServer: {}\nNo hover information available.",
            server_name
        ));
    }

    let hover: HoverResult =
        serde_json::from_value(value.clone()).context("failed to parse hover result")?;
    let rendered = render_hover_contents(&hover.contents);
    if rendered.trim().is_empty() {
        return Ok(format!(
            "Operation: hover\nServer: {}\nNo hover information available.",
            server_name
        ));
    }

    Ok(format!(
        "Operation: hover\nServer: {}\n\n{}",
        server_name,
        rendered.trim()
    ))
}

fn format_document_symbols(
    server_name: &str,
    value: &Value,
    workspace_root: &Path,
) -> Result<String> {
    let mut out = format!("Operation: document_symbols\nServer: {}", server_name);
    if value.is_null() {
        out.push_str("\nNo symbols found.");
        return Ok(out);
    }

    if let Ok(symbols) = serde_json::from_value::<Vec<DocumentSymbol>>(value.clone()) {
        if symbols.is_empty() {
            out.push_str("\nNo symbols found.");
            return Ok(out);
        }
        out.push_str(&format!("\nResults: {}", count_document_symbols(&symbols)));
        append_document_symbols(&mut out, &symbols, 0);
        return Ok(out);
    }

    let infos: Vec<SymbolInformation> =
        serde_json::from_value(value.clone()).context("failed to parse document symbol result")?;
    if infos.is_empty() {
        out.push_str("\nNo symbols found.");
        return Ok(out);
    }
    out.push_str(&format!("\nResults: {}", infos.len()));
    for (index, symbol) in infos.iter().enumerate() {
        out.push_str(&format!(
            "\n{}. {} [{}] - {}",
            index + 1,
            symbol.name,
            symbol_kind(symbol.kind),
            format_location_line(&symbol.location, workspace_root)
        ));
    }
    Ok(out)
}

fn format_workspace_symbols(
    query: &str,
    batch: WorkspaceSymbolBatch,
    workspace_root: &Path,
) -> Result<String> {
    let mut out = format!("Operation: workspace_symbols\nQuery: {}", query);

    let mut total = 0usize;
    for (server_name, value) in batch.results {
        let symbols = parse_workspace_symbols(&value)?;
        if symbols.is_empty() {
            continue;
        }
        total += symbols.len();
        out.push_str(&format!(
            "\n\nServer: {}\nResults: {}",
            server_name,
            symbols.len()
        ));
        for (index, symbol) in symbols.iter().enumerate() {
            out.push_str(&format!(
                "\n{}. {} [{}] - {}{}",
                index + 1,
                symbol.name,
                symbol_kind(symbol.kind),
                render_workspace_symbol_location(&symbol.location, workspace_root),
                symbol
                    .container_name
                    .as_deref()
                    .filter(|name| !name.is_empty())
                    .map(|name| format!(" (container: {})", name))
                    .unwrap_or_default(),
            ));
        }
    }

    if total == 0 {
        out.push_str("\nNo symbols found.");
    }

    if !batch.errors.is_empty() {
        out.push_str("\n\nWarnings:");
        for error in batch.errors {
            out.push_str(&format!("\n- {}", error));
        }
    }

    Ok(out)
}

fn format_call_hierarchy(
    server_name: &str,
    direction: CallHierarchyDirection,
    value: &Value,
    workspace_root: &Path,
) -> Result<String> {
    match direction {
        CallHierarchyDirection::Incoming => {
            let calls: Vec<IncomingCall> = serde_json::from_value(value.clone())
                .context("failed to parse incoming call hierarchy")?;
            let mut out = format!(
                "Operation: call_hierarchy\nServer: {}\nDirection: incoming",
                server_name
            );
            if calls.is_empty() {
                out.push_str("\nNo incoming calls found.");
                return Ok(out);
            }
            out.push_str(&format!("\nResults: {}", calls.len()));
            for (index, call) in calls.iter().enumerate() {
                let line = call
                    .from_ranges
                    .first()
                    .map(|range| range.start.line + 1)
                    .unwrap_or(call.from.range.start.line + 1);
                out.push_str(&format!(
                    "\n{}. {} [{}] - {}:{}:{}",
                    index + 1,
                    call.from.name,
                    symbol_kind(call.from.kind),
                    display_path_for_uri(&call.from.uri, workspace_root),
                    line,
                    call.from.range.start.character + 1,
                ));
            }
            Ok(out)
        }
        CallHierarchyDirection::Outgoing => {
            let calls: Vec<OutgoingCall> = serde_json::from_value(value.clone())
                .context("failed to parse outgoing call hierarchy")?;
            let mut out = format!(
                "Operation: call_hierarchy\nServer: {}\nDirection: outgoing",
                server_name
            );
            if calls.is_empty() {
                out.push_str("\nNo outgoing calls found.");
                return Ok(out);
            }
            out.push_str(&format!("\nResults: {}", calls.len()));
            for (index, call) in calls.iter().enumerate() {
                let line = call
                    .from_ranges
                    .first()
                    .map(|range| range.start.line + 1)
                    .unwrap_or(call.to.range.start.line + 1);
                out.push_str(&format!(
                    "\n{}. {} [{}] - {}:{}:{}",
                    index + 1,
                    call.to.name,
                    symbol_kind(call.to.kind),
                    display_path_for_uri(&call.to.uri, workspace_root),
                    line,
                    call.to.range.start.character + 1,
                ));
            }
            Ok(out)
        }
    }
}

fn parse_workspace_symbols(value: &Value) -> Result<Vec<WorkspaceSymbol>> {
    if value.is_null() {
        return Ok(Vec::new());
    }

    if let Ok(symbols) = serde_json::from_value::<Vec<WorkspaceSymbol>>(value.clone()) {
        return Ok(symbols);
    }

    let infos: Vec<SymbolInformation> =
        serde_json::from_value(value.clone()).context("failed to parse workspace symbol result")?;
    Ok(infos
        .into_iter()
        .map(|info| WorkspaceSymbol {
            name: info.name,
            kind: info.kind,
            location: WorkspaceSymbolLocation::Location(info.location),
            container_name: info.container_name,
        })
        .collect())
}

fn format_location_line(location: &Location, workspace_root: &Path) -> String {
    let path = path_for_uri(&location.uri, workspace_root);
    let line = location.range.start.line + 1;
    let character = location.range.start.character + 1;
    let snippet = line_snippet(&path, line)
        .map(|snippet| format!(" - {}", snippet))
        .unwrap_or_default();
    format!(
        "{}:{}:{}{}",
        display_path(&path, workspace_root),
        line,
        character,
        snippet
    )
}

fn path_for_uri(uri: &str, workspace_root: &Path) -> PathBuf {
    let raw = file_path_from_uri(uri).unwrap_or_else(|_| PathBuf::from(uri));
    resolve_workspace_path(raw, workspace_root)
}

fn display_path_for_uri(uri: &str, workspace_root: &Path) -> String {
    let path = path_for_uri(uri, workspace_root);
    display_path(&path, workspace_root)
}

fn display_path(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn resolve_workspace_path(path: PathBuf, workspace_root: &Path) -> PathBuf {
    if path.strip_prefix(workspace_root).is_ok() || path.exists() {
        return path;
    }

    remap_to_workspace(&path, workspace_root).unwrap_or(path)
}

fn remap_to_workspace(path: &Path, workspace_root: &Path) -> Option<PathBuf> {
    let normal_components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();

    for start in 0..normal_components.len() {
        let mut candidate = workspace_root.to_path_buf();
        for component in &normal_components[start..] {
            candidate.push(component);
        }
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn line_snippet(path: &Path, line_number: u32) -> Option<String> {
    let line_index = usize::try_from(line_number.saturating_sub(1)).ok()?;
    let content = fs::read_to_string(path).ok()?;
    let line = content.lines().nth(line_index)?.trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

fn render_hover_contents(contents: &HoverContents) -> String {
    match contents {
        HoverContents::Scalar(text) => text.clone(),
        HoverContents::Markup(markup) => {
            if markup.kind.as_deref() == Some("markdown") {
                markup.value.trim().to_string()
            } else {
                markup.value.clone()
            }
        }
        HoverContents::Array(items) => items
            .iter()
            .map(render_hover_contents)
            .filter(|item| !item.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::LanguageString(map) => map
            .get("value")
            .cloned()
            .or_else(|| map.get("language").cloned())
            .unwrap_or_default(),
    }
}

fn append_document_symbols(out: &mut String, symbols: &[DocumentSymbol], depth: usize) {
    for symbol in symbols {
        out.push_str(&format!(
            "\n{}- {} [{}] @ {}:{}",
            "  ".repeat(depth),
            symbol.name,
            symbol_kind(symbol.kind),
            symbol.range.start.line + 1,
            symbol.range.start.character + 1,
        ));
        append_document_symbols(out, &symbol.children, depth + 1);
    }
}

fn count_document_symbols(symbols: &[DocumentSymbol]) -> usize {
    symbols
        .iter()
        .map(|symbol| 1 + count_document_symbols(&symbol.children))
        .sum()
}

fn render_workspace_symbol_location(
    location: &WorkspaceSymbolLocation,
    workspace_root: &Path,
) -> String {
    match location {
        WorkspaceSymbolLocation::Location(location) => {
            format_location_line(location, workspace_root)
        }
        WorkspaceSymbolLocation::UriOnly { uri } => display_path_for_uri(uri, workspace_root),
    }
}

fn symbol_kind(kind: u32) -> &'static str {
    match kind {
        1 => "File",
        2 => "Module",
        3 => "Namespace",
        4 => "Package",
        5 => "Class",
        6 => "Method",
        7 => "Property",
        8 => "Field",
        9 => "Constructor",
        10 => "Enum",
        11 => "Interface",
        12 => "Function",
        13 => "Variable",
        14 => "Constant",
        15 => "String",
        16 => "Number",
        17 => "Boolean",
        18 => "Array",
        19 => "Object",
        20 => "Key",
        21 => "Null",
        22 => "EnumMember",
        23 => "Struct",
        24 => "Event",
        25 => "Operator",
        26 => "TypeParameter",
        _ => "Unknown",
    }
}

impl From<LocationLink> for Location {
    fn from(value: LocationLink) -> Self {
        Self {
            uri: value.target_uri,
            range: value.target_selection_range.unwrap_or(value.target_range),
        }
    }
}

trait JsonValueExt {
    fn tap_mut(self, update: impl FnOnce(&mut Value)) -> Value;
}

impl JsonValueExt for Value {
    fn tap_mut(mut self, update: impl FnOnce(&mut Value)) -> Value {
        update(&mut self);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn tool_input_requires_known_operation() {
        let err = ToolInput::from_value(json!({"operation":"rename"})).unwrap_err();
        assert!(err.to_string().contains("unknown LSP operation"));
    }

    #[test]
    fn parse_location_targets_accepts_location_links() {
        let value = json!([
            {
                "targetUri": "file:///tmp/main.rs",
                "targetRange": {"start": {"line": 1, "character": 2}},
                "targetSelectionRange": {"start": {"line": 3, "character": 4}}
            }
        ]);
        let locations = parse_location_targets(&value).unwrap();
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].range.start.line, 3);
        assert_eq!(locations[0].range.start.character, 4);
    }

    #[test]
    fn render_hover_contents_flattens_nested_values() {
        let contents = HoverContents::Array(vec![
            HoverContents::Scalar("fn demo()".to_string()),
            HoverContents::Markup(MarkupContent {
                kind: Some("markdown".to_string()),
                value: "Returns a value".to_string(),
            }),
        ]);
        let rendered = render_hover_contents(&contents);
        assert!(rendered.contains("fn demo()"));
        assert!(rendered.contains("Returns a value"));
    }

    #[test]
    fn format_location_line_includes_source_snippet() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("lib.rs");
        fs::write(&file, "first\nlet value = demo();\n").unwrap();
        let uri = crate::services::lsp::types::file_uri(&file).unwrap();
        let location = Location {
            uri,
            range: Range {
                start: Position {
                    line: 1,
                    character: 4,
                },
            },
        };

        let rendered = format_location_line(&location, temp.path());
        assert!(rendered.contains("lib.rs:2:5"));
        assert!(rendered.contains("let value = demo();"));
    }

    #[test]
    fn format_location_line_remaps_container_style_prefixes() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("src/main.rs");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "fn main() {}\n").unwrap();

        let location = Location {
            uri: "file:///app/src/main.rs".to_string(),
            range: Range {
                start: Position {
                    line: 0,
                    character: 3,
                },
            },
        };

        let rendered = format_location_line(&location, temp.path());
        assert!(rendered.contains("src/main.rs:1:4"));
        assert!(!rendered.contains("/app/"));
        assert!(rendered.contains("fn main() {}"));
    }
}
