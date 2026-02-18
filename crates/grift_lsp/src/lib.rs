#![forbid(unsafe_code)]

//! # Grift LSP
//!
//! A Language Server Protocol implementation for the Grift Lisp language.
//! Provides diagnostics, hover documentation, and completion for builtins.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use grift::Lisp;
use grift_arena::ArenaError;
use serde::{Deserialize, Serialize};

// ── Builtin documentation database ──────────────────────────────────────────

/// Documentation entry for a builtin or keyword.
pub struct DocEntry {
    pub signature: &'static str,
    pub description: &'static str,
    pub kind: CompletionItemKind,
}

/// Build the documentation database from LANGUAGE.md content.
pub fn builtin_docs() -> HashMap<&'static str, DocEntry> {
    let mut m = HashMap::new();

    // Operatives (keywords)
    m.insert(
        "quote",
        DocEntry {
            signature: "(quote expr)",
            description: "Return expr without evaluating it.",
            kind: CompletionItemKind::Keyword,
        },
    );
    m.insert("if", DocEntry {
        signature: "(if test consequent [alternative])",
        description: "Evaluate test. If #t, evaluate consequent; if #f, evaluate alternative (or return () if omitted). test must be a boolean.",
        kind: CompletionItemKind::Keyword,
    });
    m.insert("define!", DocEntry {
        signature: "(define! definiend expression)",
        description: "Evaluate expression, then match definiend (a parameter tree) against the result, binding symbols in the current environment. Returns #inert.",
        kind: CompletionItemKind::Keyword,
    });
    m.insert("set!", DocEntry {
        signature: "(set! env-expr definiend expression)",
        description: "Evaluate env-expr to get a target environment and expression to get a value, then bind definiend in the target environment. Returns #inert.",
        kind: CompletionItemKind::Keyword,
    });
    m.insert("lambda", DocEntry {
        signature: "(lambda params body ...)",
        description: "Create an applicative (arguments are evaluated before binding). Equivalent to (wrap (vau params #ignore (begin body ...))).",
        kind: CompletionItemKind::Keyword,
    });
    m.insert("vau", DocEntry {
        signature: "(vau params env-param body ...)",
        description: "Create an operative (fexpr). params is matched against unevaluated operands. env-param is bound to the caller's environment (or #ignore).",
        kind: CompletionItemKind::Keyword,
    });
    m.insert("begin", DocEntry {
        signature: "(begin expr1 expr2 ... exprN)",
        description: "Evaluate each expression in order. Returns the value of the last expression, or () if given no expressions.",
        kind: CompletionItemKind::Keyword,
    });
    m.insert("cond", DocEntry {
        signature: "(cond (test1 body1 ...) ... (else bodyN ...))",
        description: "Evaluate tests in order until one returns #t (or else clause), then evaluate the corresponding body. Returns () if no clause matches.",
        kind: CompletionItemKind::Keyword,
    });
    m.insert("and", DocEntry {
        signature: "(and expr1 expr2 ... exprN)",
        description: "Evaluate left-to-right. If any evaluates to #f, return #f immediately. Otherwise return the last result. With no arguments, returns #t.",
        kind: CompletionItemKind::Keyword,
    });
    m.insert("or", DocEntry {
        signature: "(or expr1 expr2 ... exprN)",
        description: "Evaluate left-to-right. If any evaluates to #t, return #t immediately. Otherwise return the last result. With no arguments, returns #f.",
        kind: CompletionItemKind::Keyword,
    });
    m.insert("let", DocEntry {
        signature: "(let ((name1 val1) (name2 val2) ...) body ...)",
        description: "Create a child environment, evaluate each val in the outer environment, bind names in the child, then evaluate body in the child.",
        kind: CompletionItemKind::Keyword,
    });

    // Applicatives (functions)
    m.insert(
        "cons",
        DocEntry {
            signature: "(cons a b)",
            description: "Construct a pair.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "+",
        DocEntry {
            signature: "(+ . numbers)",
            description: "Sum. Zero arguments returns 0.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "-",
        DocEntry {
            signature: "(- n . rest)",
            description: "With one argument: negate. With two+: left fold subtraction.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "*",
        DocEntry {
            signature: "(* . numbers)",
            description: "Product. Zero arguments returns 1.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "/",
        DocEntry {
            signature: "(/ a b)",
            description: "Integer (truncating) division. DivisionByZero if b is 0.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "=",
        DocEntry {
            signature: "(= a b)",
            description: "Numeric equality. Both arguments must be numbers.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "<",
        DocEntry {
            signature: "(< a b)",
            description: "Less than. Both arguments must be numbers.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        ">",
        DocEntry {
            signature: "(> a b)",
            description: "Greater than. Both arguments must be numbers.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "<=",
        DocEntry {
            signature: "(<= a b)",
            description: "Less than or equal. Both arguments must be numbers.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        ">=",
        DocEntry {
            signature: "(>= a b)",
            description: "Greater than or equal. Both arguments must be numbers.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "car",
        DocEntry {
            signature: "(car pair)",
            description: "First element of a pair.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "cdr",
        DocEntry {
            signature: "(cdr pair)",
            description: "Second element of a pair.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "list",
        DocEntry {
            signature: "(list . items)",
            description: "Return the argument list as-is (already a proper list).",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "null?",
        DocEntry {
            signature: "(null? . objects)",
            description: "Returns #t if all arguments are ().",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "not",
        DocEntry {
            signature: "(not boolean)",
            description: "Boolean negation. Argument must be a boolean.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "pair?",
        DocEntry {
            signature: "(pair? . objects)",
            description: "Returns #t if all arguments are pairs.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "number?",
        DocEntry {
            signature: "(number? . objects)",
            description: "Returns #t if all arguments are numbers.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "symbol?",
        DocEntry {
            signature: "(symbol? . objects)",
            description: "Returns #t if all arguments are symbols.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "boolean?",
        DocEntry {
            signature: "(boolean? . objects)",
            description: "Returns #t if all arguments are booleans.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "inert?",
        DocEntry {
            signature: "(inert? . objects)",
            description: "Returns #t if all arguments are #inert.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "ignore?",
        DocEntry {
            signature: "(ignore? . objects)",
            description: "Returns #t if all arguments are #ignore.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert("eq?", DocEntry {
        signature: "(eq? a b)",
        description: "Identity equality. Compares by value for scalars, by arena identity for constructed types.",
        kind: CompletionItemKind::Function,
    });
    m.insert("equal?", DocEntry {
        signature: "(equal? a b)",
        description: "Structural equality. Returns #t whenever eq? would, plus compares pairs recursively and strings character-by-character.",
        kind: CompletionItemKind::Function,
    });
    m.insert("eval", DocEntry {
        signature: "(eval expr [env])",
        description: "Evaluate expr in the given environment (defaults to the standard environment if omitted).",
        kind: CompletionItemKind::Function,
    });
    m.insert(
        "wrap",
        DocEntry {
            signature: "(wrap combiner)",
            description: "Wrap a combiner in an applicative (arguments will be evaluated).",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "unwrap",
        DocEntry {
            signature: "(unwrap applicative)",
            description: "Extract the underlying combiner from an applicative.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "operative?",
        DocEntry {
            signature: "(operative? . objects)",
            description: "Returns #t if all arguments are operatives or builtins.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "applicative?",
        DocEntry {
            signature: "(applicative? . objects)",
            description: "Returns #t if all arguments are applicatives.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert("make-environment", DocEntry {
        signature: "(make-environment . envs)",
        description: "Create a new environment with the given parents. All arguments must be environments.",
        kind: CompletionItemKind::Function,
    });
    m.insert(
        "make-empty-environment",
        DocEntry {
            signature: "(make-empty-environment)",
            description: "Create a new environment with no parents.",
            kind: CompletionItemKind::Function,
        },
    );
    m.insert(
        "environment?",
        DocEntry {
            signature: "(environment? . objects)",
            description: "Returns #t if all arguments are environments.",
            kind: CompletionItemKind::Function,
        },
    );

    m
}

// ── LSP JSON-RPC types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RpcMessage {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    pub method: Option<String>,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
}

// ── LSP domain types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Option<u32>,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct PublishDiagnosticsParams {
    pub uri: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentIdentifier {
    pub uri: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentItem {
    pub uri: String,
    pub language_id: String,
    pub version: i64,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidOpenParams {
    pub text_document: TextDocumentItem,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidChangeParams {
    pub text_document: VersionedTextDocumentIdentifier,
    pub content_changes: Vec<TextDocumentContentChangeEvent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionedTextDocumentIdentifier {
    pub uri: String,
    pub version: i64,
}

#[derive(Debug, Deserialize)]
pub struct TextDocumentContentChangeEvent {
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoverParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
}

#[derive(Debug, Serialize)]
pub struct Hover {
    pub contents: MarkupContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

#[derive(Debug, Serialize)]
pub struct MarkupContent {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct CompletionItemKind(u32);

#[allow(dead_code, non_upper_case_globals)]
impl CompletionItemKind {
    pub const Function: Self = Self(3);
    pub const Keyword: Self = Self(14);
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    pub label: String,
    pub kind: Option<CompletionItemKind>,
    pub detail: Option<String>,
    pub documentation: Option<MarkupContent>,
}

// ── Language Server ─────────────────────────────────────────────────────────

pub struct GriftLanguageServer {
    docs: HashMap<&'static str, DocEntry>,
    documents: HashMap<String, String>,
}

impl GriftLanguageServer {
    pub fn new() -> Self {
        Self {
            docs: builtin_docs(),
            documents: HashMap::new(),
        }
    }

    /// Handle the `initialize` request and return the server capabilities.
    pub fn initialize(&self, id: Option<serde_json::Value>) -> RpcResponse {
        let capabilities = serde_json::json!({
            "capabilities": {
                "textDocumentSync": 1,
                "hoverProvider": true,
                "completionProvider": {
                    "triggerCharacters": ["(", " "]
                }
            },
            "serverInfo": {
                "name": "grift-lsp",
                "version": env!("CARGO_PKG_VERSION")
            }
        });
        RpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(capabilities),
            error: None,
        }
    }

    /// Handle `textDocument/didOpen`.
    pub fn did_open(&mut self, params: DidOpenParams) -> Option<RpcNotification> {
        let uri = params.text_document.uri.clone();
        let text = params.text_document.text;
        self.documents.insert(uri.clone(), text.clone());
        Some(self.publish_diagnostics(&uri, &text))
    }

    /// Handle `textDocument/didChange` (full sync).
    pub fn did_change(&mut self, params: DidChangeParams) -> Option<RpcNotification> {
        let uri = params.text_document.uri.clone();
        if let Some(change) = params.content_changes.into_iter().last() {
            self.documents.insert(uri.clone(), change.text.clone());
            Some(self.publish_diagnostics(&uri, &change.text))
        } else {
            None
        }
    }

    /// Run parse check and produce a diagnostics notification.
    fn publish_diagnostics(&self, uri: &str, text: &str) -> RpcNotification {
        let diagnostics = self.check_parse(text);
        RpcNotification {
            jsonrpc: "2.0".into(),
            method: "textDocument/publishDiagnostics".into(),
            params: serde_json::to_value(PublishDiagnosticsParams {
                uri: uri.to_string(),
                diagnostics,
            })
            .unwrap_or_default(),
        }
    }

    /// Attempt to parse the document and collect diagnostics.
    fn check_parse(&self, text: &str) -> Vec<Diagnostic> {
        let lisp: Lisp<20000> = Lisp::new();
        match lisp.eval(text) {
            Ok(_) => Vec::new(),
            Err(ArenaError::ParseError) => {
                vec![Diagnostic {
                    range: Range {
                        start: Position {
                            line: 0,
                            character: 0,
                        },
                        end: Position {
                            line: 0,
                            character: text.lines().next().map_or(0, |l| l.len() as u32),
                        },
                    },
                    severity: Some(1), // Error
                    message: "Parse error: malformed S-expression".into(),
                }]
            }
            Err(e) => {
                // Non-parse errors mean parsing succeeded; report as warnings
                // only if they're evaluation-time issues. For a pure parse check
                // we could ignore these, but surfacing them is helpful.
                vec![Diagnostic {
                    range: Range {
                        start: Position {
                            line: 0,
                            character: 0,
                        },
                        end: Position {
                            line: 0,
                            character: text.lines().next().map_or(0, |l| l.len() as u32),
                        },
                    },
                    severity: Some(2), // Warning
                    message: format!("{}", e),
                }]
            }
        }
    }

    /// Handle `textDocument/hover`.
    pub fn handle_hover(&self, params: HoverParams) -> Option<Hover> {
        let text = self.documents.get(&params.text_document.uri)?;
        let word = word_at_position(text, &params.position)?;
        let entry = self.docs.get(word.as_str())?;
        Some(Hover {
            contents: MarkupContent {
                kind: "markdown".into(),
                value: format!("```lisp\n{}\n```\n\n{}", entry.signature, entry.description),
            },
            range: None,
        })
    }

    /// Handle `textDocument/completion`.
    pub fn handle_completion(&self) -> Vec<CompletionItem> {
        self.docs
            .iter()
            .map(|(name, entry)| CompletionItem {
                label: (*name).to_string(),
                kind: Some(entry.kind),
                detail: Some(entry.signature.to_string()),
                documentation: Some(MarkupContent {
                    kind: "markdown".into(),
                    value: entry.description.to_string(),
                }),
            })
            .collect()
    }

    /// Handle `shutdown`.
    pub fn shutdown(&self, id: Option<serde_json::Value>) -> RpcResponse {
        RpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::Value::Null),
            error: None,
        }
    }
}

impl Default for GriftLanguageServer {
    fn default() -> Self {
        Self::new()
    }
}

// ── Utilities ───────────────────────────────────────────────────────────────

/// Extract the word (symbol) at a given position in the text.
fn word_at_position(text: &str, pos: &Position) -> Option<String> {
    let line = text.lines().nth(pos.line as usize)?;
    let col = pos.character as usize;
    if col > line.len() {
        return None;
    }

    let bytes = line.as_bytes();
    let is_symbol_char =
        |b: u8| !matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'(' | b')' | b'"' | b';');

    let mut start = col;
    while start > 0 && is_symbol_char(bytes[start - 1]) {
        start -= 1;
    }

    let mut end = col;
    while end < bytes.len() && is_symbol_char(bytes[end]) {
        end += 1;
    }

    if start == end {
        return None;
    }
    Some(line[start..end].to_string())
}

// ── LSP message framing (Content-Length) ────────────────────────────────────

/// Read one LSP message from stdin using Content-Length framing.
pub fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<RpcMessage>> {
    let mut content_length: Option<usize> = None;

    // Read headers
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Ok(None); // EOF
        }
        let header = header.trim();
        if header.is_empty() {
            break; // End of headers
        }
        if let Some(len_str) = header.strip_prefix("Content-Length:") {
            content_length = len_str.trim().parse().ok();
        }
    }

    let length = content_length.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "Missing Content-Length header")
    })?;

    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;

    let msg: RpcMessage =
        serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(msg))
}

/// Write an LSP message to stdout with Content-Length framing.
pub fn write_message<W: Write>(writer: &mut W, json: &[u8]) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n", json.len())?;
    writer.write_all(json)?;
    writer.flush()
}

/// Serialize and send an RPC response.
pub fn send_response<W: Write>(writer: &mut W, response: &RpcResponse) -> io::Result<()> {
    let json = serde_json::to_vec(response).map_err(io::Error::other)?;
    write_message(writer, &json)
}

/// Serialize and send an RPC notification.
pub fn send_notification<W: Write>(
    writer: &mut W,
    notification: &RpcNotification,
) -> io::Result<()> {
    let json = serde_json::to_vec(notification).map_err(io::Error::other)?;
    write_message(writer, &json)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_docs_populated() {
        let docs = builtin_docs();
        assert!(docs.contains_key("quote"));
        assert!(docs.contains_key("+"));
        assert!(docs.contains_key("lambda"));
        assert!(docs.contains_key("cons"));
        assert!(docs.contains_key("make-environment"));
    }

    #[test]
    fn test_word_at_position() {
        let text = "(define! x 42)";
        let word = word_at_position(
            text,
            &Position {
                line: 0,
                character: 3,
            },
        );
        assert_eq!(word, Some("define!".into()));

        let word = word_at_position(
            text,
            &Position {
                line: 0,
                character: 0,
            },
        );
        assert_eq!(word, None); // on '('
    }

    #[test]
    fn test_diagnostics_valid() {
        let server = GriftLanguageServer::new();
        let diags = server.check_parse("(+ 1 2)");
        assert!(diags.is_empty() || diags[0].severity == Some(2));
    }

    #[test]
    fn test_diagnostics_parse_error() {
        let server = GriftLanguageServer::new();
        let diags = server.check_parse("(+ 1 2");
        assert!(!diags.is_empty());
        assert_eq!(diags[0].severity, Some(1));
    }

    #[test]
    fn test_completion_returns_all_builtins() {
        let server = GriftLanguageServer::new();
        let items = server.handle_completion();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"define!"));
        assert!(labels.contains(&"car"));
        assert!(labels.contains(&"null?"));
    }

    #[test]
    fn test_hover_known_symbol() {
        let mut server = GriftLanguageServer::new();
        server
            .documents
            .insert("file:///test.grift".into(), "(cons 1 2)".into());
        let hover = server.handle_hover(HoverParams {
            text_document: TextDocumentIdentifier {
                uri: "file:///test.grift".into(),
            },
            position: Position {
                line: 0,
                character: 2,
            },
        });
        assert!(hover.is_some());
        assert!(hover.unwrap().contents.value.contains("cons"));
    }

    #[test]
    fn test_hover_unknown_symbol() {
        let mut server = GriftLanguageServer::new();
        server
            .documents
            .insert("file:///test.grift".into(), "(foo 1 2)".into());
        let hover = server.handle_hover(HoverParams {
            text_document: TextDocumentIdentifier {
                uri: "file:///test.grift".into(),
            },
            position: Position {
                line: 0,
                character: 2,
            },
        });
        assert!(hover.is_none());
    }

    #[test]
    fn test_read_message() {
        let input = b"Content-Length: 58\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}";
        let mut cursor = io::Cursor::new(input.as_ref());
        let msg = read_message(&mut cursor).unwrap().unwrap();
        assert_eq!(msg.method.as_deref(), Some("initialize"));
        assert_eq!(msg.id, Some(serde_json::json!(1)));
    }

    #[test]
    fn test_initialize_response() {
        let server = GriftLanguageServer::new();
        let resp = server.initialize(Some(serde_json::json!(1)));
        assert!(resp.result.is_some());
        let caps = resp.result.unwrap();
        assert!(caps["capabilities"]["hoverProvider"].as_bool().unwrap());
    }
}
