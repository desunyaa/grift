#![forbid(unsafe_code)]

//! # Grift Check — Static Analysis for Grift Lisp
//!
//! Provides static verification of Grift source code without executing it.
//!
//! ## Features
//!
//! - **Syntax checking**: Validates S-expression syntax (balanced parens, valid tokens)
//! - **Bracket matching**: Reports unmatched parentheses with positions
//! - **String validation**: Detects unterminated string literals
//! - **Diagnostics**: Returns structured diagnostic messages with severity and location
//! - **Documentation database**: Structured docs for all builtins, shared by REPL and LSP

pub mod docs;

/// Severity level for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// An error that prevents the code from running.
    Error,
    /// A warning about potential issues.
    Warning,
    /// An informational hint.
    Hint,
}

/// A source position (0-based line and column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// 0-based line number.
    pub line: usize,
    /// 0-based column (byte offset within the line).
    pub col: usize,
}

/// A diagnostic message produced by the checker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The severity of this diagnostic.
    pub severity: Severity,
    /// Start position in the source.
    pub start: Position,
    /// End position in the source.
    pub end: Position,
    /// The diagnostic message.
    pub message: String,
}

/// Result of checking a source file.
#[derive(Debug)]
pub struct CheckResult {
    /// All diagnostics found.
    pub diagnostics: Vec<Diagnostic>,
}

impl CheckResult {
    /// Returns true if no errors were found.
    pub fn is_ok(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Returns only errors.
    pub fn errors(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect()
    }

    /// Returns only warnings.
    pub fn warnings(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect()
    }
}

/// Check grift source code for errors.
///
/// Performs the following checks:
/// 1. Bracket matching (unmatched parentheses)
/// 2. String literal validation (unterminated strings)
///
/// # Example
///
/// ```
/// use grift_check::{check, Severity};
///
/// let result = check("(+ 1 2)");
/// assert!(result.is_ok());
///
/// let result = check("(+ 1 2");
/// assert!(!result.is_ok());
/// assert_eq!(result.errors().len(), 1);
/// ```
pub fn check(source: &str) -> CheckResult {
    let mut diagnostics = Vec::new();

    // Check bracket matching
    check_brackets(source, &mut diagnostics);

    // Check string literals
    check_strings(source, &mut diagnostics);

    // Only run arity checks if there are no structural errors,
    // since unmatched brackets would confuse the token scanner.
    if !diagnostics.iter().any(|d| d.severity == Severity::Error) {
        check_form_arity(source, &mut diagnostics);
    }

    CheckResult { diagnostics }
}

/// Convert a byte offset to a Position (line, col).
fn offset_to_position(source: &str, offset: usize) -> Position {
    let mut line = 0;
    let mut col = 0;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Position { line, col }
}

/// Check for unmatched parentheses.
fn check_brackets(source: &str, diagnostics: &mut Vec<Diagnostic>) {
    let mut stack: Vec<usize> = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b';' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'(' => {
                stack.push(i);
                i += 1;
            }
            b')' => {
                if stack.pop().is_none() {
                    let pos = offset_to_position(source, i);
                    diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        start: pos,
                        end: Position {
                            line: pos.line,
                            col: pos.col + 1,
                        },
                        message: "Unmatched closing parenthesis".to_string(),
                    });
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    for &open_offset in &stack {
        let pos = offset_to_position(source, open_offset);
        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            start: pos,
            end: Position {
                line: pos.line,
                col: pos.col + 1,
            },
            message: "Unmatched opening parenthesis".to_string(),
        });
    }
}

/// Check for unterminated string literals.
fn check_strings(source: &str, diagnostics: &mut Vec<Diagnostic>) {
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b';' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                let start_offset = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i >= bytes.len() {
                    let pos = offset_to_position(source, start_offset);
                    diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        start: pos,
                        end: Position {
                            line: pos.line,
                            col: pos.col + 1,
                        },
                        message: "Unterminated string literal".to_string(),
                    });
                } else {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
}

/// Minimum argument count for forms where arity can be checked statically.
/// Returns `(form_name, min_args)` or None if the form doesn't have a
/// statically checkable minimum.
fn min_arity(name: &str) -> Option<(&str, usize)> {
    match name {
        "define!" => Some(("define!", 2)),
        "set!" => Some(("set!", 3)),
        "lambda" => Some(("lambda", 2)),
        "vau" => Some(("vau", 3)),
        "if" => Some(("if", 2)),
        "let" => Some(("let", 2)),
        _ => None,
    }
}

/// Check that well-known forms have the minimum number of arguments.
///
/// Scans the source for `(form-name ...)` patterns and counts top-level
/// tokens inside the parentheses. This is a lightweight, token-level
/// check—not a full parse—so it only catches obvious arity errors.
fn check_form_arity(source: &str, diagnostics: &mut Vec<Diagnostic>) {
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b';' => {
                // Skip comment
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                // Skip string
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'(' => {
                let form_start = i;
                i += 1;

                // Skip whitespace after '('
                while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
                    i += 1;
                }

                // Read the form name
                let name_start = i;
                while i < bytes.len()
                    && !matches!(
                        bytes[i],
                        b' ' | b'\t' | b'\n' | b'\r' | b'(' | b')' | b'"' | b';'
                    )
                {
                    i += 1;
                }
                let name = &source[name_start..i];

                if let Some((form_name, min)) = min_arity(name) {
                    // Count top-level arguments (tokens and sub-expressions)
                    let mut arg_count = 0usize;
                    let mut depth = 0i32;

                    while i < bytes.len() && !(depth == 0 && bytes[i] == b')') {
                        match bytes[i] {
                            b';' => {
                                while i < bytes.len() && bytes[i] != b'\n' {
                                    i += 1;
                                }
                            }
                            b'"' => {
                                if depth == 0 {
                                    arg_count += 1;
                                }
                                i += 1;
                                while i < bytes.len() && bytes[i] != b'"' {
                                    if bytes[i] == b'\\' {
                                        i += 1;
                                    }
                                    i += 1;
                                }
                                if i < bytes.len() {
                                    i += 1;
                                }
                            }
                            b'(' => {
                                if depth == 0 {
                                    arg_count += 1;
                                }
                                depth += 1;
                                i += 1;
                            }
                            b')' => {
                                depth -= 1;
                                i += 1;
                            }
                            b' ' | b'\t' | b'\n' | b'\r' => {
                                i += 1;
                            }
                            _ => {
                                // Start of a token
                                if depth == 0 {
                                    arg_count += 1;
                                }
                                while i < bytes.len()
                                    && !matches!(
                                        bytes[i],
                                        b' ' | b'\t' | b'\n' | b'\r' | b'(' | b')' | b'"' | b';'
                                    )
                                {
                                    i += 1;
                                }
                            }
                        }
                    }

                    if arg_count < min {
                        let pos = offset_to_position(source, form_start);
                        diagnostics.push(Diagnostic {
                            severity: Severity::Warning,
                            start: pos,
                            end: Position {
                                line: pos.line,
                                col: pos.col + name.len() + 1,
                            },
                            message: format!(
                                "`{form_name}` expects at least {min} argument{}, got {arg_count}",
                                if min == 1 { "" } else { "s" }
                            ),
                        });
                    }
                }
                // Don't advance i here — outer loop will continue from current position
            }
            _ => {
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_expression() {
        let result = check("(+ 1 2)");
        assert!(result.is_ok());
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_unmatched_open_paren() {
        let result = check("(+ 1 2");
        assert!(!result.is_ok());
        assert_eq!(result.errors().len(), 1);
        assert!(
            result.errors()[0]
                .message
                .contains("Unmatched opening parenthesis")
        );
    }

    #[test]
    fn test_unmatched_close_paren() {
        let result = check("(+ 1 2))");
        assert!(!result.is_ok());
        assert_eq!(result.errors().len(), 1);
        assert!(
            result.errors()[0]
                .message
                .contains("Unmatched closing parenthesis")
        );
    }

    #[test]
    fn test_unterminated_string() {
        let result = check("\"hello");
        assert!(!result.is_ok());
        assert!(result.errors()[0].message.contains("Unterminated string"));
    }

    #[test]
    fn test_nested_parens_valid() {
        let result = check("(define! fib (lambda (n a b) (if (= n 0) a (fib (- n 1) b (+ a b)))))");
        assert!(result.is_ok());
    }

    #[test]
    fn test_comment_ignored() {
        let result = check("; this is a comment\n(+ 1 2)");
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_expressions() {
        let result = check("(define! x 1)\n(+ x 2)");
        assert!(result.is_ok());
    }

    #[test]
    fn test_empty_input() {
        let result = check("");
        assert!(result.is_ok());
    }

    #[test]
    fn test_position_tracking() {
        let result = check("(+ 1 2)\n)");
        assert!(!result.is_ok());
        let err = result.errors()[0];
        assert_eq!(err.start.line, 1);
        assert_eq!(err.start.col, 0);
    }

    #[test]
    fn test_string_in_parens_ok() {
        let result = check("(define! x \"hello world\")");
        assert!(result.is_ok());
    }

    // ── Arity check tests ───────────────────────────────────────────────

    #[test]
    fn test_define_valid_arity() {
        let result = check("(define! x 42)");
        assert!(result.is_ok());
    }

    #[test]
    fn test_define_no_args() {
        let result = check("(define!)");
        assert_eq!(result.warnings().len(), 1);
        assert!(result.warnings()[0].message.contains("define!"));
        assert!(result.warnings()[0].message.contains("at least 2"));
    }

    #[test]
    fn test_define_one_arg() {
        let result = check("(define! x)");
        assert_eq!(result.warnings().len(), 1);
        assert!(result.warnings()[0].message.contains("define!"));
    }

    #[test]
    fn test_lambda_valid() {
        let result = check("(lambda (x) (+ x 1))");
        assert!(result.is_ok());
    }

    #[test]
    fn test_lambda_no_args() {
        let result = check("(lambda)");
        assert_eq!(result.warnings().len(), 1);
        assert!(result.warnings()[0].message.contains("lambda"));
    }

    #[test]
    fn test_if_valid() {
        let result = check("(if #t 1 2)");
        assert!(result.is_ok());
    }

    #[test]
    fn test_if_no_args() {
        let result = check("(if)");
        assert_eq!(result.warnings().len(), 1);
        assert!(result.warnings()[0].message.contains("if"));
    }

    #[test]
    fn test_nested_forms_ok() {
        let result = check("(define! fib (lambda (n a b) (if (= n 0) a (fib (- n 1) b (+ a b)))))");
        assert!(result.is_ok());
    }
}
