//! Error types for the pure evaluator.

/// Error kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    UnboundVariable,
    TypeError,
    WrongArgCount,
    DivisionByZero,
    Internal,
    Malformed,
    MatchFailure,
}

/// Evaluation error.
#[derive(Debug, Clone, Copy)]
pub struct EvalError {
    pub kind: ErrorKind,
}

impl EvalError {
    /// Create a new error.
    pub const fn new(kind: ErrorKind) -> Self {
        EvalError { kind }
    }
}

/// Result type alias.
pub type EvalResult = Result<(), EvalError>;
