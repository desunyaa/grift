//! # Lisp REPL
//!
//! A Read-Eval-Print-Loop for the classic Lisp interpreter.
//!
//! ## Features
//!
//! - Rich error display with stack traces
//! - GC commands and statistics
//! - Help system
//!
//! ## Usage
//!
//! ```rust,ignore
//! use lisp_repl::run_repl;
//!
//! run_repl::<10000>();
//! ```

use std::io::{self, BufRead, Write};

pub use lisp_eval::{
    Arena, ArenaIndex, ArenaError, ArenaResult, Trace, GcStats,
    Value, Builtin, Lisp, ParseError, ParseErrorKind, SourceLoc, parse,
    EvalError, EvalResult, Evaluator, ErrorKind, StackFrame,
};

// ============================================================================
// Value Formatting
// ============================================================================

/// Format a Lisp value as a string
pub fn format_value<const N: usize>(lisp: &Lisp<N>, idx: ArenaIndex, buf: &mut String) {
    format_value_impl(lisp, idx, buf, 0)
}

/// Format with depth limit for protection against cycles
fn format_value_impl<const N: usize>(
    lisp: &Lisp<N>, 
    idx: ArenaIndex, 
    buf: &mut String,
    depth: usize,
) {
    if depth > 100 {
        buf.push_str("...");
        return;
    }
    
    match lisp.get(idx) {
        Ok(Value::Nil) => buf.push_str("()"),
        Ok(Value::True) => buf.push_str("#t"),
        Ok(Value::False) => buf.push_str("#f"),
        Ok(Value::Number(n)) => {
            use std::fmt::Write;
            write!(buf, "{}", n).unwrap();
        }
        Ok(Value::Char(c)) => {
            buf.push_str("#\\");
            match c {
                ' ' => buf.push_str("space"),
                '\n' => buf.push_str("newline"),
                '\t' => buf.push_str("tab"),
                _ => buf.push(c),
            }
        }
        Ok(Value::Symbol { chars }) => {
            format_char_list(lisp, chars, buf);
        }
        Ok(Value::Cons { .. }) => {
            buf.push('(');
            format_list_contents(lisp, idx, buf, depth + 1);
            buf.push(')');
        }
        Ok(Value::Lambda { .. }) => {
            buf.push_str("#<lambda>");
        }
        Ok(Value::Thunk { cached, .. }) => {
            if cached.is_null() {
                buf.push_str("#<promise>");
            } else {
                buf.push_str("#<promise:forced>");
            }
        }
        Ok(Value::Builtin(b)) => {
            buf.push_str("#<builtin:");
            buf.push_str(b.name());
            buf.push('>');
        }
        Err(_) => buf.push_str("#<error>"),
    }
}

/// Format the contents of a list (without outer parens)
fn format_list_contents<const N: usize>(
    lisp: &Lisp<N>, 
    mut idx: ArenaIndex, 
    buf: &mut String,
    depth: usize,
) {
    let mut first = true;
    let mut count = 0;
    
    loop {
        if count > 100 {
            buf.push_str(" ...");
            break;
        }
        
        match lisp.get(idx) {
            Ok(Value::Nil) => break,
            Ok(Value::Cons { car, cdr }) => {
                if !first {
                    buf.push(' ');
                }
                first = false;
                format_value_impl(lisp, car, buf, depth);
                idx = cdr;
                count += 1;
            }
            Ok(_) => {
                // Improper list (dotted pair)
                buf.push_str(" . ");
                format_value_impl(lisp, idx, buf, depth);
                break;
            }
            Err(_) => {
                buf.push_str(" . #<error>");
                break;
            }
        }
    }
}

/// Format a char list (symbol name)
fn format_char_list<const N: usize>(lisp: &Lisp<N>, mut idx: ArenaIndex, buf: &mut String) {
    let mut count = 0;
    loop {
        if count > 64 {
            buf.push_str("...");
            break;
        }
        match lisp.get(idx) {
            Ok(Value::Nil) => break,
            Ok(Value::Cons { car, cdr }) => {
                if let Ok(Value::Char(c)) = lisp.get(car) {
                    buf.push(c);
                }
                idx = cdr;
                count += 1;
            }
            _ => break,
        }
    }
}

/// Convert a Lisp value to a string
pub fn value_to_string<const N: usize>(lisp: &Lisp<N>, idx: ArenaIndex) -> String {
    let mut buf = String::new();
    format_value(lisp, idx, &mut buf);
    buf
}

// ============================================================================
// Error Formatting
// ============================================================================

/// Format an evaluation error with full context
pub fn format_error<const N: usize>(lisp: &Lisp<N>, err: &EvalError) -> String {
    let mut buf = String::new();
    
    // Main error message
    buf.push_str("Error: ");
    buf.push_str(err.kind.as_str());
    
    // Additional context based on error type
    match err.kind {
        ErrorKind::UnboundVariable => {
            if !err.expr.is_null() {
                buf.push_str(": ");
                format_value(lisp, err.expr, &mut buf);
            }
        }
        ErrorKind::TypeError => {
            if let (Some(expected), Some(got)) = (err.expected, err.got) {
                use std::fmt::Write;
                write!(buf, ": expected {}, got {}", expected, got).unwrap();
            }
        }
        ErrorKind::WrongArgCount => {
            if let (Some(expected), Some(got)) = (err.expected_args, err.got_args) {
                use std::fmt::Write;
                write!(buf, ": expected {} arguments, got {}", expected, got).unwrap();
            }
        }
        ErrorKind::Parse => {
            if let Some(ref pe) = err.parse_error {
                use std::fmt::Write;
                write!(buf, " at line {}, column {}: ", pe.loc.line, pe.loc.column).unwrap();
                match pe.kind {
                    ParseErrorKind::UnexpectedEof => buf.push_str("unexpected end of input"),
                    ParseErrorKind::UnexpectedChar(c) => {
                        write!(buf, "unexpected character '{}'", c).unwrap();
                    }
                    ParseErrorKind::UnmatchedParen => buf.push_str("unmatched parenthesis"),
                    ParseErrorKind::NumberOverflow => buf.push_str("number too large"),
                    ParseErrorKind::OutOfMemory => buf.push_str("out of memory"),
                    ParseErrorKind::InvalidHashLiteral => buf.push_str("invalid # literal"),
                }
            }
        }
        ErrorKind::UserError => {
            if !err.expr.is_null() {
                buf.push_str(": ");
                format_value(lisp, err.expr, &mut buf);
            }
        }
        _ => {
            // Include expression if available
            if !err.expr.is_null() && !matches!(err.kind, ErrorKind::OutOfMemory | ErrorKind::StackOverflow) {
                buf.push_str(" in: ");
                let mut expr_buf = String::new();
                format_value(lisp, err.expr, &mut expr_buf);
                // Truncate long expressions
                if expr_buf.len() > 60 {
                    buf.push_str(&expr_buf[..57]);
                    buf.push_str("...");
                } else {
                    buf.push_str(&expr_buf);
                }
            }
        }
    }
    
    // Custom message if present
    let msg = err.message.as_str();
    if !msg.is_empty() {
        buf.push_str("\n  ");
        buf.push_str(msg);
    }
    
    // Stack trace
    if err.backtrace_len > 0 {
        buf.push_str("\n\nStack trace (most recent call first):");
        for i in (0..err.backtrace_len).rev() {
            let frame = &err.backtrace[i];
            buf.push_str("\n  ");
            use std::fmt::Write;
            write!(buf, "{}: ", err.backtrace_len - i).unwrap();
            
            if !frame.func.is_null() {
                let mut func_buf = String::new();
                format_value(lisp, frame.func, &mut func_buf);
                if func_buf.len() > 40 {
                    buf.push_str(&func_buf[..37]);
                    buf.push_str("...");
                } else {
                    buf.push_str(&func_buf);
                }
            } else if !frame.expr.is_null() {
                let mut expr_buf = String::new();
                format_value(lisp, frame.expr, &mut expr_buf);
                if expr_buf.len() > 40 {
                    buf.push_str(&expr_buf[..37]);
                    buf.push_str("...");
                } else {
                    buf.push_str(&expr_buf);
                }
            } else {
                buf.push_str("<unknown>");
            }
        }
    }
    
    buf
}

// ============================================================================
// REPL
// ============================================================================

/// The REPL structure (for API convenience)
pub struct Repl<const N: usize> {
    lisp: Lisp<N>,
}

impl<const N: usize> Repl<N> {
    /// Create a new REPL
    pub fn new() -> Self {
        Repl {
            lisp: Lisp::new(),
        }
    }
    
    /// Get the Lisp context
    pub fn lisp(&self) -> &Lisp<N> {
        &self.lisp
    }
}

impl<const N: usize> Default for Repl<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Run a REPL session
pub fn run_repl<const N: usize>() {
    let lisp: Lisp<N> = Lisp::new();
    let mut eval = match Evaluator::new(&lisp) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Failed to initialize evaluator: {}", format_error(&lisp, &e));
            return;
        }
    };
    
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    
    println!("Classic Lisp (pwn_arena)");
    println!("========================");
    println!("Features: TCO, call-by-need, full mutation, rich errors");
    println!("Truthiness: only #f is false (nil/'() are truthy!)");
    println!("Type :help for commands, Ctrl+D to exit.");
    println!("Arena capacity: {} cells", N);
    println!();
    
    let mut input_buffer = String::new();
    let mut continuation = false;
    
    loop {
        if continuation {
            print!("... ");
        } else {
            print!("> ");
        }
        stdout.flush().unwrap();
        
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                // EOF
                println!("\nGoodbye!");
                break;
            }
            Ok(_) => {
                if continuation {
                    input_buffer.push_str(&line);
                } else {
                    input_buffer = line;
                }
                
                let input = input_buffer.trim();
                if input.is_empty() {
                    continuation = false;
                    input_buffer.clear();
                    continue;
                }
                
                // Check for unbalanced parens (simple continuation)
                let open = input.chars().filter(|&c| c == '(').count();
                let close = input.chars().filter(|&c| c == ')').count();
                if open > close {
                    continuation = true;
                    continue;
                }
                
                continuation = false;
                
                // Special commands
                if input.starts_with(':') {
                    if handle_command(input, &lisp, &mut eval) {
                        input_buffer.clear();
                        continue;
                    }
                    // If handle_command returns false, it's :quit
                    break;
                }
                
                // Evaluate
                match eval.eval_str(input) {
                    Ok(result) => {
                        println!("{}", value_to_string(&lisp, result));
                    }
                    Err(e) => {
                        println!("{}", format_error(&lisp, &e));
                    }
                }
                
                input_buffer.clear();
            }
            Err(e) => {
                eprintln!("Read error: {}", e);
                break;
            }
        }
    }
}

/// Handle REPL commands. Returns true to continue, false to quit.
fn handle_command<const N: usize>(input: &str, lisp: &Lisp<N>, eval: &mut Evaluator<N>) -> bool {
    let cmd = input.trim();
    
    match cmd {
        ":q" | ":quit" | ":exit" => {
            println!("Goodbye!");
            return false;
        }
        ":gc" => {
            let stats = eval.gc();
            println!("GC complete:");
            println!("  Marked:    {}", stats.marked);
            println!("  Collected: {}", stats.collected);
            println!("  Before:    {}", stats.total_before);
        }
        ":stats" | ":stat" => {
            let stats = lisp.stats();
            println!("Arena statistics:");
            println!("  Capacity:      {}", stats.capacity);
            println!("  Allocated:     {}", stats.allocated);
            println!("  Free:          {}", stats.capacity - stats.allocated);
            println!("  Usage:         {:.1}%", stats.usage_percent());
            println!("  Fragmentation: {:.2}", stats.fragmentation);
        }
        ":help" | ":h" | ":?" => {
            print_help();
        }
        ":env" => {
            println!("Global environment has {} bindings", 
                     count_env(lisp, eval.global_env()));
        }
        _ if cmd.starts_with(":load ") => {
            println!("File loading not implemented in this version");
        }
        _ => {
            println!("Unknown command: {}", cmd);
            println!("Type :help for available commands");
        }
    }
    
    true
}

fn count_env<const N: usize>(lisp: &Lisp<N>, mut env: ArenaIndex) -> usize {
    let mut count = 0;
    loop {
        match lisp.get(env) {
            Ok(Value::Nil) => return count,
            Ok(Value::Cons { cdr, .. }) => {
                count += 1;
                env = cdr;
            }
            _ => return count,
        }
    }
}

fn print_help() {
    println!("Classic Lisp Help");
    println!("=================");
    println!();
    println!("Truthiness:");
    println!("  Only #f is false. Everything else is truthy, including:");
    println!("  - nil / '() (empty list)");
    println!("  - 0 (zero)");
    println!("  - \"\" (empty string, if supported)");
    println!();
    println!("Literals:");
    println!("  #t, #f      - Boolean true and false");
    println!("  42, -10     - Numbers");
    println!("  'symbol     - Quoted symbol");
    println!("  '(1 2 3)    - Quoted list");
    println!();
    println!("Special Forms:");
    println!("  (quote x) or 'x       - Return x unevaluated");
    println!("  (if cond then else)   - Conditional (TCO in branches)");
    println!("  (cond (c1 e1)...)     - Multi-way conditional");
    println!("  (lambda (args) body)  - Create closure");
    println!("  (define name val)     - Define variable");
    println!("  (define (f x) body)   - Define function");
    println!("  (let ((x v)...) body) - Parallel local bindings");
    println!("  (let* ((x v)...) body)- Sequential local bindings");
    println!("  (begin e1 e2...)      - Sequence (TCO in last)");
    println!("  (and e1 e2...)        - Short-circuit and");
    println!("  (or e1 e2...)         - Short-circuit or");
    println!("  (delay expr)          - Create lazy thunk (call-by-need)");
    println!();
    println!("Built-in Functions:");
    println!("  List:   car, cdr, cons, list");
    println!("  Pred:   atom, eq, null?, pair?, number?, boolean?");
    println!("          symbol?, procedure?, promise?");
    println!("  Lazy:   force, delay");
    println!("  Bool:   not");
    println!("  Math:   +, -, *, /, mod");
    println!("  Cmp:    <, >, <=, >=, =");
    println!("  I/O:    print, display, newline");
    println!("  Err:    error");
    println!();
    println!("NOTE: This is a PURE functional Lisp - no mutation!");
    println!("      Call-by-need (delay/force) is semantically sound.");
    println!();
    println!("REPL Commands:");
    println!("  :help, :h, :?  - Show this help");
    println!("  :gc            - Run garbage collection");
    println!("  :stats         - Show arena statistics");
    println!("  :env           - Show environment size");
    println!("  :quit, :q      - Exit");
    println!();
    println!("Examples:");
    println!("  (define (fact n) (if (= n 0) 1 (* n (fact (- n 1)))))");
    println!("  (fact 10)");
    println!();
    println!("  (define lazy-fib (delay (fib 20)))");
    println!("  (force lazy-fib)  ; computes once, memoizes");
    println!();
}

/// Evaluate a string and return the result as a string
pub fn eval_to_string<const N: usize>(lisp: &Lisp<N>, eval: &mut Evaluator<N>, input: &str) -> Result<String, EvalError> {
    let result = eval.eval_str(input)?;
    Ok(value_to_string(lisp, result))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_format_number() {
        let lisp: Lisp<100> = Lisp::new();
        let idx = lisp.number(42).unwrap();
        assert_eq!(value_to_string(&lisp, idx), "42");
    }
    
    #[test]
    fn test_format_booleans() {
        let lisp: Lisp<100> = Lisp::new();
        
        let t = lisp.true_val().unwrap();
        assert_eq!(value_to_string(&lisp, t), "#t");
        
        let f = lisp.false_val().unwrap();
        assert_eq!(value_to_string(&lisp, f), "#f");
    }
    
    #[test]
    fn test_format_nil() {
        let lisp: Lisp<100> = Lisp::new();
        let idx = lisp.nil().unwrap();
        assert_eq!(value_to_string(&lisp, idx), "()");
    }
    
    #[test]
    fn test_format_symbol() {
        let lisp: Lisp<100> = Lisp::new();
        let idx = lisp.symbol("hello").unwrap();
        assert_eq!(value_to_string(&lisp, idx), "hello");
    }
    
    #[test]
    fn test_format_list() {
        let lisp: Lisp<100> = Lisp::new();
        let a = lisp.number(1).unwrap();
        let b = lisp.number(2).unwrap();
        let c = lisp.number(3).unwrap();
        let nil = lisp.nil().unwrap();
        let list = lisp.cons(c, nil).unwrap();
        let list = lisp.cons(b, list).unwrap();
        let list = lisp.cons(a, list).unwrap();
        assert_eq!(value_to_string(&lisp, list), "(1 2 3)");
    }
    
    #[test]
    fn test_format_dotted_pair() {
        let lisp: Lisp<100> = Lisp::new();
        let a = lisp.number(1).unwrap();
        let b = lisp.number(2).unwrap();
        let pair = lisp.cons(a, b).unwrap();
        assert_eq!(value_to_string(&lisp, pair), "(1 . 2)");
    }
    
    #[test]
    fn test_format_thunk() {
        let lisp: Lisp<100> = Lisp::new();
        let expr = lisp.number(42).unwrap();
        let env = lisp.nil().unwrap();
        let thunk = lisp.thunk(expr, env).unwrap();
        assert_eq!(value_to_string(&lisp, thunk), "#<promise>");
    }
    
    #[test]
    fn test_eval_and_format() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert_eq!(eval_to_string(&lisp, &mut eval, "(+ 1 2)").unwrap(), "3");
        assert_eq!(eval_to_string(&lisp, &mut eval, "'(a b c)").unwrap(), "(a b c)");
        assert_eq!(eval_to_string(&lisp, &mut eval, "#t").unwrap(), "#t");
        assert_eq!(eval_to_string(&lisp, &mut eval, "#f").unwrap(), "#f");
    }
    
    #[test]
    fn test_factorial() {
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (fact n) (if (= n 0) 1 (* n (fact (- n 1)))))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(fact 10)").unwrap(), "3628800");
    }
    
    #[test]
    fn test_tco_recursion() {
        let lisp: Lisp<5000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // TCO test - would overflow without proper tail call optimization
        eval.eval_str("(define (sum-to n acc) (if (= n 0) acc (sum-to (- n 1) (+ acc n))))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(sum-to 50 0)").unwrap(), "1275");
    }
    
    #[test]
    fn test_fibonacci() {
        let lisp: Lisp<5000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (fib n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2)))))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(fib 10)").unwrap(), "55");
    }
    
    #[test]
    fn test_higher_order() {
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (twice f x) (f (f x)))").unwrap();
        eval.eval_str("(define (add1 x) (+ x 1))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(twice add1 5)").unwrap(), "7");
    }
    
    #[test]
    fn test_closures() {
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (make-adder n) (lambda (x) (+ x n)))").unwrap();
        eval.eval_str("(define add5 (make-adder 5))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(add5 10)").unwrap(), "15");
    }
    
    #[test]
    fn test_list_operations() {
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Define length
        eval.eval_str("(define (length lst) (if (null? lst) 0 (+ 1 (length (cdr lst)))))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(length '(1 2 3 4 5))").unwrap(), "5");
        
        // Define append
        eval.eval_str("(define (append a b) (if (null? a) b (cons (car a) (append (cdr a) b))))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(append '(1 2) '(3 4))").unwrap(), "(1 2 3 4)");
        
        // Define reverse
        eval.eval_str("(define (reverse-helper lst acc) (if (null? lst) acc (reverse-helper (cdr lst) (cons (car lst) acc))))").unwrap();
        eval.eval_str("(define (reverse lst) (reverse-helper lst '()))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(reverse '(1 2 3))").unwrap(), "(3 2 1)");
    }
    
    #[test]
    fn test_map() {
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (map f lst) (if (null? lst) '() (cons (f (car lst)) (map f (cdr lst)))))").unwrap();
        eval.eval_str("(define (square x) (* x x))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(map square '(1 2 3 4))").unwrap(), "(1 4 9 16)");
    }
    
    #[test]
    fn test_filter() {
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (filter pred lst) (cond ((null? lst) '()) ((pred (car lst)) (cons (car lst) (filter pred (cdr lst)))) (else (filter pred (cdr lst)))))").unwrap();
        eval.eval_str("(define (even x) (= (mod x 2) 0))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(filter even '(1 2 3 4 5 6))").unwrap(), "(2 4 6)");
    }
    
    #[test]
    fn test_fold() {
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (fold f acc lst) (if (null? lst) acc (fold f (f acc (car lst)) (cdr lst))))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(fold + 0 '(1 2 3 4 5))").unwrap(), "15");
    }
    
    // NOTE: test_mutation removed - this is a PURE Lisp!
    
    #[test]
    fn test_delay_force() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define p (delay (+ 1 2)))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(promise? p)").unwrap(), "#t");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force p)").unwrap(), "3");
    }
    
    #[test]
    fn test_thunk_memoization() {
        // In a pure language, memoization is semantically transparent
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define lazy-val (delay (* 6 7)))").unwrap();
        
        // Force multiple times - all return same result
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force lazy-val)").unwrap(), "42");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force lazy-val)").unwrap(), "42");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force lazy-val)").unwrap(), "42");
    }
    
    #[test]
    fn test_thunk_force_non_promise() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Force on non-thunks returns the value unchanged
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force 42)").unwrap(), "42");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force 'hello)").unwrap(), "hello");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force '(1 2 3))").unwrap(), "(1 2 3)");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force #t)").unwrap(), "#t");
    }
    
    #[test]
    fn test_thunk_captures_lexical_env() {
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Thunk captures the lexical environment at creation time
        // In a pure language, this is straightforward - the value never changes
        eval.eval_str("(define x 10)").unwrap();
        eval.eval_str("(define lazy-x (delay x))").unwrap();
        
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force lazy-x)").unwrap(), "10");
        
        // Multiple forces return same value (referential transparency)
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force lazy-x)").unwrap(), "10");
    }
    
    #[test]
    fn test_thunk_in_let_binding() {
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Thunk created in let captures the let's environment
        eval.eval_str("(define (make-lazy n) (let ((x n)) (delay (+ x 100))))").unwrap();
        eval.eval_str("(define lazy-110 (make-lazy 10))").unwrap();
        eval.eval_str("(define lazy-200 (make-lazy 100))").unwrap();
        
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force lazy-110)").unwrap(), "110");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force lazy-200)").unwrap(), "200");
    }
    
    #[test]
    fn test_thunk_nested() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Nested delays require nested forces
        eval.eval_str("(define nested (delay (delay (delay 42))))").unwrap();
        
        assert_eq!(eval_to_string(&lisp, &mut eval, "(promise? nested)").unwrap(), "#t");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(promise? (force nested))").unwrap(), "#t");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(promise? (force (force nested)))").unwrap(), "#t");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force (force (force nested)))").unwrap(), "42");
    }
    
    #[test]
    fn test_thunk_lazy_computation() {
        // In a pure language, lazy computation is semantically transparent
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Create lazy values with pure computations
        eval.eval_str("(define lazy-a (delay (* 1 1)))").unwrap();
        eval.eval_str("(define lazy-b (delay (* 2 2)))").unwrap();
        eval.eval_str("(define lazy-c (delay (* 3 3)))").unwrap();
        
        // Force in any order - referentially transparent
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force lazy-b)").unwrap(), "4");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force lazy-a)").unwrap(), "1");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force lazy-c)").unwrap(), "9");
        
        // Force again - same results
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force lazy-b)").unwrap(), "4");
    }
    
    #[test]
    fn test_thunk_lazy_if() {
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // A lazy-if that only evaluates the selected branch
        eval.eval_str("(define (lazy-if cond then-thunk else-thunk) (force (if cond then-thunk else-thunk)))").unwrap();
        
        // Create thunks for branches
        eval.eval_str("(define t-branch (delay 'then))").unwrap();
        eval.eval_str("(define e-branch (delay 'else))").unwrap();
        
        // Only then should execute
        assert_eq!(eval_to_string(&lisp, &mut eval, "(lazy-if #t t-branch e-branch)").unwrap(), "then");
        
        // Only else should execute
        assert_eq!(eval_to_string(&lisp, &mut eval, "(lazy-if #f t-branch e-branch)").unwrap(), "else");
    }
    
    #[test]
    fn test_thunk_in_list() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // List of thunks
        eval.eval_str("(define thunk-list (list (delay 1) (delay 2) (delay 3)))").unwrap();
        
        // All elements are promises
        assert_eq!(eval_to_string(&lisp, &mut eval, "(promise? (car thunk-list))").unwrap(), "#t");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(promise? (car (cdr thunk-list)))").unwrap(), "#t");
        
        // Force them
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force (car thunk-list))").unwrap(), "1");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force (car (cdr thunk-list)))").unwrap(), "2");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(force (car (cdr (cdr thunk-list))))").unwrap(), "3");
    }
    
    #[test]
    fn test_thunk_simple_stream() {
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Define stream operations
        eval.eval_str("(define (stream-cons x s) (cons x (delay s)))").unwrap();
        eval.eval_str("(define (stream-car s) (car s))").unwrap();
        eval.eval_str("(define (stream-cdr s) (force (cdr s)))").unwrap();
        
        // Finite stream: (1 2 3)
        eval.eval_str("(define s3 (stream-cons 3 '()))").unwrap();
        eval.eval_str("(define s2 (stream-cons 2 s3))").unwrap();
        eval.eval_str("(define s1 (stream-cons 1 s2))").unwrap();
        
        // Access elements lazily
        assert_eq!(eval_to_string(&lisp, &mut eval, "(stream-car s1)").unwrap(), "1");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(stream-car (stream-cdr s1))").unwrap(), "2");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(stream-car (stream-cdr (stream-cdr s1)))").unwrap(), "3");
    }
    
    #[test]
    fn test_thunk_cyclic_stream() {
        // In a pure language, we can create infinite streams using recursion and delay
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Stream utilities
        eval.eval_str("(define (stream-car s) (car s))").unwrap();
        eval.eval_str("(define (stream-cdr s) (force (cdr s)))").unwrap();
        
        // Create an infinite stream of ones using a generator function
        eval.eval_str("(define (make-ones) (cons 1 (delay (make-ones))))").unwrap();
        eval.eval_str("(define ones (make-ones))").unwrap();
        
        // Access elements - infinite stream of 1s
        assert_eq!(eval_to_string(&lisp, &mut eval, "(stream-car ones)").unwrap(), "1");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(stream-car (stream-cdr ones))").unwrap(), "1");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(stream-car (stream-cdr (stream-cdr ones)))").unwrap(), "1");
    }
    
    #[test]
    fn test_thunk_format_display() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Unforced thunk displays as promise
        eval.eval_str("(define p (delay 42))").unwrap();
        let unforced = eval_to_string(&lisp, &mut eval, "p").unwrap();
        assert!(unforced.contains("promise"));
        
        // Force it
        eval.eval_str("(force p)").unwrap();
        
        // Forced thunk might display differently (implementation detail)
        // But promise? should still return true
        assert_eq!(eval_to_string(&lisp, &mut eval, "(promise? p)").unwrap(), "#t");
    }
    
    #[test]
    fn test_nil_is_truthy() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // nil is truthy
        assert_eq!(eval_to_string(&lisp, &mut eval, "(if nil 'yes 'no)").unwrap(), "yes");
        assert_eq!(eval_to_string(&lisp, &mut eval, "(if '() 'yes 'no)").unwrap(), "yes");
        
        // Only #f is false
        assert_eq!(eval_to_string(&lisp, &mut eval, "(if #f 'yes 'no)").unwrap(), "no");
    }
}
