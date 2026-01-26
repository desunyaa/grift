//! # Lisp REPL
//!
//! A Read-Eval-Print-Loop for the classic Lisp interpreter.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use lisp_repl::Repl;
//!
//! let mut repl = Repl::<10000>::new().unwrap();
//! repl.run();
//! ```

use std::io::{self, BufRead, Write};

pub use lisp_eval::{
    Arena, ArenaIndex, ArenaError, ArenaResult, Trace, GcStats,
    Value, Builtin, Lisp, ParseError, parse,
    EvalError, EvalResult, Evaluator,
};

/// Format a Lisp value as a string
pub fn format_value<const N: usize>(lisp: &Lisp<N>, idx: ArenaIndex, buf: &mut String) {
    match lisp.get(idx) {
        Ok(Value::Nil) => buf.push_str("nil"),
        Ok(Value::Number(n)) => {
            use std::fmt::Write;
            write!(buf, "{}", n).unwrap();
        }
        Ok(Value::Char(c)) => {
            buf.push_str("#\\");
            buf.push(c);
        }
        Ok(Value::Symbol { chars }) => {
            format_char_list(lisp, chars, buf);
        }
        Ok(Value::Cons { .. }) => {
            buf.push('(');
            format_list_contents(lisp, idx, buf);
            buf.push(')');
        }
        Ok(Value::Lambda { .. }) => {
            buf.push_str("#<lambda>");
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
fn format_list_contents<const N: usize>(lisp: &Lisp<N>, mut idx: ArenaIndex, buf: &mut String) {
    let mut first = true;
    
    loop {
        match lisp.get(idx) {
            Ok(Value::Nil) => break,
            Ok(Value::Cons { car, cdr }) => {
                if !first {
                    buf.push(' ');
                }
                first = false;
                format_value(lisp, car, buf);
                idx = cdr;
            }
            Ok(_) => {
                // Improper list (dotted pair)
                buf.push_str(" . ");
                format_value(lisp, idx, buf);
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
    loop {
        match lisp.get(idx) {
            Ok(Value::Nil) => break,
            Ok(Value::Cons { car, cdr }) => {
                if let Ok(Value::Char(c)) = lisp.get(car) {
                    buf.push(c);
                }
                idx = cdr;
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
            eprintln!("Failed to initialize evaluator: {:?}", e);
            return;
        }
    };
    
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    
    println!("Classic Lisp (pwn_arena)");
    println!("Type expressions to evaluate. Use Ctrl+D to exit.");
    println!("Arena capacity: {} cells", N);
    println!();
    
    loop {
        print!("> ");
        stdout.flush().unwrap();
        
        let mut input = String::new();
        match stdin.lock().read_line(&mut input) {
            Ok(0) => {
                // EOF
                println!("\nGoodbye!");
                break;
            }
            Ok(_) => {
                let input = input.trim();
                if input.is_empty() {
                    continue;
                }
                
                // Special commands
                if input == ":q" || input == ":quit" {
                    println!("Goodbye!");
                    break;
                }
                if input == ":gc" {
                    let stats = eval.gc();
                    println!("GC: marked={}, collected={}, before={}", 
                             stats.marked, stats.collected, stats.total_before);
                    continue;
                }
                if input == ":stats" {
                    let stats = lisp.stats();
                    println!("Arena: {}/{} used ({:.1}%), fragmentation={:.2}", 
                             stats.allocated, stats.capacity, 
                             stats.usage_percent(), stats.fragmentation);
                    continue;
                }
                if input == ":help" || input == ":?" {
                    print_help();
                    continue;
                }
                
                // Evaluate
                match eval.eval_str(input) {
                    Ok(result) => {
                        println!("{}", value_to_string(&lisp, result));
                    }
                    Err(e) => {
                        println!("Error: {:?}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Read error: {}", e);
                break;
            }
        }
    }
}

fn print_help() {
    println!("Classic Lisp Help");
    println!("=================");
    println!();
    println!("Special Forms:");
    println!("  (quote x) or 'x     - Return x unevaluated");
    println!("  (if cond then else) - Conditional");
    println!("  (cond (c1 e1)...)   - Multi-way conditional");
    println!("  (lambda (args) body)- Create function");
    println!("  (define name val)   - Define variable");
    println!("  (define (f x) body) - Define function");
    println!("  (let ((x v)...) body) - Local bindings");
    println!("  (begin e1 e2...)    - Sequence");
    println!("  (set! name val)     - Mutation");
    println!("  (and e1 e2...)      - Logical and");
    println!("  (or e1 e2...)       - Logical or");
    println!();
    println!("Built-in Functions:");
    println!("  (car x)         - First element of pair");
    println!("  (cdr x)         - Rest of pair");
    println!("  (cons a b)      - Create pair");
    println!("  (list a b...)   - Create list");
    println!("  (atom x)        - Is x an atom?");
    println!("  (eq a b)        - Are a and b equal?");
    println!("  (null x)        - Is x nil?");
    println!("  (numberp x)     - Is x a number?");
    println!("  (+ a b...)      - Addition");
    println!("  (- a b...)      - Subtraction");
    println!("  (* a b...)      - Multiplication");
    println!("  (/ a b...)      - Division");
    println!("  (mod a b)       - Modulo");
    println!("  (< a b)         - Less than");
    println!("  (> a b)         - Greater than");
    println!("  (<= a b)        - Less or equal");
    println!("  (>= a b)        - Greater or equal");
    println!("  (= a b)         - Numeric equal");
    println!();
    println!("REPL Commands:");
    println!("  :help, :?       - Show this help");
    println!("  :gc             - Run garbage collection");
    println!("  :stats          - Show arena statistics");
    println!("  :quit, :q       - Exit");
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
    fn test_format_nil() {
        let lisp: Lisp<100> = Lisp::new();
        let idx = lisp.nil().unwrap();
        assert_eq!(value_to_string(&lisp, idx), "nil");
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
    fn test_eval_and_format() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert_eq!(eval_to_string(&lisp, &mut eval, "(+ 1 2)").unwrap(), "3");
        assert_eq!(eval_to_string(&lisp, &mut eval, "'(a b c)").unwrap(), "(a b c)");
    }
    
    #[test]
    fn test_factorial() {
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (fact n) (if (= n 0) 1 (* n (fact (- n 1)))))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(fact 10)").unwrap(), "3628800");
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
        eval.eval_str("(define (length lst) (if (null lst) 0 (+ 1 (length (cdr lst)))))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(length '(1 2 3 4 5))").unwrap(), "5");
        
        // Define append
        eval.eval_str("(define (append a b) (if (null a) b (cons (car a) (append (cdr a) b))))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(append '(1 2) '(3 4))").unwrap(), "(1 2 3 4)");
        
        // Define reverse
        eval.eval_str("(define (reverse-helper lst acc) (if (null lst) acc (reverse-helper (cdr lst) (cons (car lst) acc))))").unwrap();
        eval.eval_str("(define (reverse lst) (reverse-helper lst nil))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(reverse '(1 2 3))").unwrap(), "(3 2 1)");
    }
    
    #[test]
    fn test_map() {
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (map f lst) (if (null lst) nil (cons (f (car lst)) (map f (cdr lst)))))").unwrap();
        eval.eval_str("(define (square x) (* x x))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(map square '(1 2 3 4))").unwrap(), "(1 4 9 16)");
    }
    
    #[test]
    fn test_filter() {
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (filter pred lst) (cond ((null lst) nil) ((pred (car lst)) (cons (car lst) (filter pred (cdr lst)))) (else (filter pred (cdr lst)))))").unwrap();
        eval.eval_str("(define (even x) (= (mod x 2) 0))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(filter even '(1 2 3 4 5 6))").unwrap(), "(2 4 6)");
    }
    
    #[test]
    fn test_fold() {
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (fold f acc lst) (if (null lst) acc (fold f (f acc (car lst)) (cdr lst))))").unwrap();
        assert_eq!(eval_to_string(&lisp, &mut eval, "(fold + 0 '(1 2 3 4 5))").unwrap(), "15");
    }
}
