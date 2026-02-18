//! Builtin documentation database for the Grift language.
//!
//! This module provides structured documentation for all built-in
//! operatives and applicatives. The data is shared between the REPL
//! (for `,doc` and `,builtins` commands) and the LSP server (for
//! hover, completion, and signature help).

/// The kind of a builtin form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    /// An operative (receives unevaluated operands): quote, if, define!, etc.
    Operative,
    /// An applicative (evaluates arguments first): +, cons, car, etc.
    Applicative,
}

/// Documentation entry for a builtin or keyword.
#[derive(Debug)]
pub struct DocEntry {
    /// The call signature, e.g. `"(cons a b)"`.
    pub signature: &'static str,
    /// A plain-text description of what the form does.
    pub description: &'static str,
    /// Whether this is an operative or applicative.
    pub kind: DocKind,
}

/// Build the documentation database for all Grift builtins.
///
/// Returns a list of `(name, DocEntry)` pairs, sorted by name.
///
/// # Example
///
/// ```
/// let docs = grift_check::docs::builtin_docs();
/// let lambda = docs.iter().find(|(n, _)| *n == "lambda").unwrap();
/// assert!(lambda.1.signature.contains("lambda"));
/// ```
pub fn builtin_docs() -> Vec<(&'static str, DocEntry)> {
    let mut docs = vec![
        // ── Operatives ──────────────────────────────────────────────────
        (
            "quote",
            DocEntry {
                signature: "(quote expr)",
                description: "Return expr without evaluating it.",
                kind: DocKind::Operative,
            },
        ),
        (
            "if",
            DocEntry {
                signature: "(if test consequent [alternative])",
                description: "Evaluate test. If #t, evaluate consequent; if #f, evaluate alternative (or return () if omitted). test must be a boolean.",
                kind: DocKind::Operative,
            },
        ),
        (
            "define!",
            DocEntry {
                signature: "(define! definiend expression)",
                description: "Evaluate expression, then match definiend (a parameter tree) against the result, binding symbols in the current environment. Returns #inert.",
                kind: DocKind::Operative,
            },
        ),
        (
            "set!",
            DocEntry {
                signature: "(set! env-expr definiend expression)",
                description: "Evaluate env-expr to get a target environment and expression to get a value, then bind definiend in the target environment. Returns #inert.",
                kind: DocKind::Operative,
            },
        ),
        (
            "lambda",
            DocEntry {
                signature: "(lambda params body ...)",
                description: "Create an applicative (arguments are evaluated before binding). Equivalent to (wrap (vau params #ignore (begin body ...))).",
                kind: DocKind::Operative,
            },
        ),
        (
            "vau",
            DocEntry {
                signature: "(vau params env-param body ...)",
                description: "Create an operative (fexpr). params is matched against unevaluated operands. env-param is bound to the caller's environment (or #ignore).",
                kind: DocKind::Operative,
            },
        ),
        (
            "begin",
            DocEntry {
                signature: "(begin expr1 expr2 ... exprN)",
                description: "Evaluate each expression in order. Returns the value of the last expression, or () if given no expressions.",
                kind: DocKind::Operative,
            },
        ),
        (
            "cond",
            DocEntry {
                signature: "(cond (test1 body1 ...) ... (else bodyN ...))",
                description: "Evaluate tests in order until one returns #t (or else clause), then evaluate the corresponding body. Returns () if no clause matches.",
                kind: DocKind::Operative,
            },
        ),
        (
            "and",
            DocEntry {
                signature: "(and expr1 expr2 ... exprN)",
                description: "Evaluate left-to-right. If any evaluates to #f, return #f immediately. Otherwise return the last result. With no arguments, returns #t.",
                kind: DocKind::Operative,
            },
        ),
        (
            "or",
            DocEntry {
                signature: "(or expr1 expr2 ... exprN)",
                description: "Evaluate left-to-right. If any evaluates to #t, return #t immediately. Otherwise return the last result. With no arguments, returns #f.",
                kind: DocKind::Operative,
            },
        ),
        (
            "let",
            DocEntry {
                signature: "(let ((name1 val1) (name2 val2) ...) body ...)",
                description: "Create a child environment, evaluate each val in the outer environment, bind names in the child, then evaluate body in the child.",
                kind: DocKind::Operative,
            },
        ),
        // ── Applicatives ────────────────────────────────────────────────
        (
            "+",
            DocEntry {
                signature: "(+ . numbers)",
                description: "Sum. Zero arguments returns 0.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "-",
            DocEntry {
                signature: "(- n . rest)",
                description: "With one argument: negate. With two+: left fold subtraction.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "*",
            DocEntry {
                signature: "(* . numbers)",
                description: "Product. Zero arguments returns 1.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "/",
            DocEntry {
                signature: "(/ a b)",
                description: "Integer (truncating) division. DivisionByZero if b is 0.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "=",
            DocEntry {
                signature: "(= a b)",
                description: "Numeric equality. Both arguments must be numbers.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "<",
            DocEntry {
                signature: "(< a b)",
                description: "Less than. Both arguments must be numbers.",
                kind: DocKind::Applicative,
            },
        ),
        (
            ">",
            DocEntry {
                signature: "(> a b)",
                description: "Greater than. Both arguments must be numbers.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "<=",
            DocEntry {
                signature: "(<= a b)",
                description: "Less than or equal. Both arguments must be numbers.",
                kind: DocKind::Applicative,
            },
        ),
        (
            ">=",
            DocEntry {
                signature: "(>= a b)",
                description: "Greater than or equal. Both arguments must be numbers.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "cons",
            DocEntry {
                signature: "(cons a b)",
                description: "Construct a pair.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "car",
            DocEntry {
                signature: "(car pair)",
                description: "First element of a pair.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "cdr",
            DocEntry {
                signature: "(cdr pair)",
                description: "Second element of a pair.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "list",
            DocEntry {
                signature: "(list . items)",
                description: "Return the argument list as-is (already a proper list).",
                kind: DocKind::Applicative,
            },
        ),
        (
            "null?",
            DocEntry {
                signature: "(null? . objects)",
                description: "Returns #t if all arguments are ().",
                kind: DocKind::Applicative,
            },
        ),
        (
            "not",
            DocEntry {
                signature: "(not boolean)",
                description: "Boolean negation. Argument must be a boolean.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "pair?",
            DocEntry {
                signature: "(pair? . objects)",
                description: "Returns #t if all arguments are pairs.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "number?",
            DocEntry {
                signature: "(number? . objects)",
                description: "Returns #t if all arguments are numbers.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "symbol?",
            DocEntry {
                signature: "(symbol? . objects)",
                description: "Returns #t if all arguments are symbols.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "boolean?",
            DocEntry {
                signature: "(boolean? . objects)",
                description: "Returns #t if all arguments are booleans.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "inert?",
            DocEntry {
                signature: "(inert? . objects)",
                description: "Returns #t if all arguments are #inert.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "ignore?",
            DocEntry {
                signature: "(ignore? . objects)",
                description: "Returns #t if all arguments are #ignore.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "eq?",
            DocEntry {
                signature: "(eq? a b)",
                description: "Identity equality. Compares by value for scalars, by arena identity for constructed types.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "equal?",
            DocEntry {
                signature: "(equal? a b)",
                description: "Structural equality. Returns #t whenever eq? would, plus compares pairs recursively and strings character-by-character.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "eval",
            DocEntry {
                signature: "(eval expr [env])",
                description: "Evaluate expr in the given environment (defaults to the standard environment if omitted).",
                kind: DocKind::Applicative,
            },
        ),
        (
            "wrap",
            DocEntry {
                signature: "(wrap combiner)",
                description: "Wrap a combiner in an applicative (arguments will be evaluated).",
                kind: DocKind::Applicative,
            },
        ),
        (
            "unwrap",
            DocEntry {
                signature: "(unwrap applicative)",
                description: "Extract the underlying combiner from an applicative.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "operative?",
            DocEntry {
                signature: "(operative? . objects)",
                description: "Returns #t if all arguments are operatives or builtins.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "applicative?",
            DocEntry {
                signature: "(applicative? . objects)",
                description: "Returns #t if all arguments are applicatives.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "make-environment",
            DocEntry {
                signature: "(make-environment . envs)",
                description: "Create a new environment with the given parents. All arguments must be environments.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "make-empty-environment",
            DocEntry {
                signature: "(make-empty-environment)",
                description: "Create a new environment with no parents.",
                kind: DocKind::Applicative,
            },
        ),
        (
            "environment?",
            DocEntry {
                signature: "(environment? . objects)",
                description: "Returns #t if all arguments are environments.",
                kind: DocKind::Applicative,
            },
        ),
    ];

    docs.sort_by_key(|(name, _)| *name);
    docs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_docs_not_empty() {
        let docs = builtin_docs();
        assert!(!docs.is_empty());
    }

    #[test]
    fn test_builtin_docs_contains_lambda() {
        let docs = builtin_docs();
        assert!(docs.iter().any(|(n, _)| *n == "lambda"));
    }

    #[test]
    fn test_builtin_docs_contains_cons() {
        let docs = builtin_docs();
        let cons = docs.iter().find(|(n, _)| *n == "cons").unwrap();
        assert_eq!(cons.1.kind, DocKind::Applicative);
        assert!(cons.1.signature.contains("cons"));
    }

    #[test]
    fn test_builtin_docs_sorted() {
        let docs = builtin_docs();
        for w in docs.windows(2) {
            assert!(w[0].0 <= w[1].0, "{} should come before {}", w[0].0, w[1].0);
        }
    }

    #[test]
    fn test_operative_count() {
        let docs = builtin_docs();
        let ops: Vec<_> = docs
            .iter()
            .filter(|(_, e)| e.kind == DocKind::Operative)
            .collect();
        assert_eq!(ops.len(), 11); // quote if define! set! lambda vau begin cond and or let
    }
}
