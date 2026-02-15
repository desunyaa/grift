//! Integration tests for grift_pure.
//!
//! These tests build ASTs manually (arena-allocated cons-list
//! s-expressions) and evaluate them through the CPS trampoline.

use grift_pure::*;
use grift_pure::intern::intern;
use grift_pure::eval::{run, eval_sequence};

// ============================================================================
// Helpers
// ============================================================================

/// Allocate a symbol value in the arena.
fn sym(arena: &Arena<Value, ARENA_SIZE>, name: &str) -> ArenaIndex {
    let s = intern(arena, name);
    arena.alloc(Value::Symbol(s)).unwrap()
}

/// Build a cons-list from a slice of indices.
fn list(arena: &Arena<Value, ARENA_SIZE>, items: &[ArenaIndex]) -> ArenaIndex {
    let mut result = arena.alloc(Value::Nil).unwrap();
    for &item in items.iter().rev() {
        result = arena.alloc(Value::Cons { head: item, tail: result }).unwrap();
    }
    result
}

/// Extract an integer from a value.
fn get_int(arena: &Arena<Value, ARENA_SIZE>, idx: ArenaIndex) -> i64 {
    let mut cur = idx;
    loop {
        match arena.get(cur).unwrap() {
            Value::Int(n) => return n,
            Value::Evaluated { value } => cur = value,
            other => panic!("expected Int, got {:?}", other),
        }
    }
}

/// Extract a boolean from a value.
fn get_bool(arena: &Arena<Value, ARENA_SIZE>, idx: ArenaIndex) -> bool {
    let mut cur = idx;
    loop {
        match arena.get(cur).unwrap() {
            Value::Bool(b) => return b,
            Value::Evaluated { value } => cur = value,
            other => panic!("expected Bool, got {:?}", other),
        }
    }
}

// ============================================================================
// Literal evaluation
// ============================================================================

#[test]
fn test_eval_int_literal() {
    let arena = new_arena();
    let env = default_env(&arena);
    let expr = arena.alloc(Value::Int(42)).unwrap();
    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 42);
}

#[test]
fn test_eval_bool_literal() {
    let arena = new_arena();
    let env = default_env(&arena);
    let expr = arena.alloc(Value::Bool(true)).unwrap();
    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_bool(&arena, result), true);
}

#[test]
fn test_eval_nil_literal() {
    let arena = new_arena();
    let env = default_env(&arena);
    let expr = arena.alloc(Value::Nil).unwrap();
    let result = run(&arena, expr, env).unwrap();
    assert!(matches!(arena.get(result).unwrap(), Value::Nil));
}

// ============================================================================
// Arithmetic
// ============================================================================

#[test]
fn test_addition() {
    // (+ 3 4) => 7
    let arena = new_arena();
    let env = default_env(&arena);

    let plus = sym(&arena, "+");
    let three = arena.alloc(Value::Int(3)).unwrap();
    let four = arena.alloc(Value::Int(4)).unwrap();
    let expr = list(&arena, &[plus, three, four]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 7);
}

#[test]
fn test_subtraction() {
    // (- 10 3) => 7
    let arena = new_arena();
    let env = default_env(&arena);

    let minus = sym(&arena, "-");
    let ten = arena.alloc(Value::Int(10)).unwrap();
    let three = arena.alloc(Value::Int(3)).unwrap();
    let expr = list(&arena, &[minus, ten, three]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 7);
}

#[test]
fn test_multiplication() {
    // (* 6 7) => 42
    let arena = new_arena();
    let env = default_env(&arena);

    let mul = sym(&arena, "*");
    let six = arena.alloc(Value::Int(6)).unwrap();
    let seven = arena.alloc(Value::Int(7)).unwrap();
    let expr = list(&arena, &[mul, six, seven]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 42);
}

#[test]
fn test_division() {
    // (/ 10 3) => 3
    let arena = new_arena();
    let env = default_env(&arena);

    let div = sym(&arena, "/");
    let ten = arena.alloc(Value::Int(10)).unwrap();
    let three = arena.alloc(Value::Int(3)).unwrap();
    let expr = list(&arena, &[div, ten, three]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 3);
}

#[test]
fn test_division_by_zero() {
    // (/ 1 0) => error
    let arena = new_arena();
    let env = default_env(&arena);

    let div = sym(&arena, "/");
    let one = arena.alloc(Value::Int(1)).unwrap();
    let zero = arena.alloc(Value::Int(0)).unwrap();
    let expr = list(&arena, &[div, one, zero]);

    let result = run(&arena, expr, env);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::DivisionByZero);
}

#[test]
fn test_modulo() {
    // (% 10 3) => 1
    let arena = new_arena();
    let env = default_env(&arena);

    let modop = sym(&arena, "%");
    let ten = arena.alloc(Value::Int(10)).unwrap();
    let three = arena.alloc(Value::Int(3)).unwrap();
    let expr = list(&arena, &[modop, ten, three]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 1);
}

#[test]
fn test_nested_arithmetic() {
    // (+ (* 2 3) (- 10 4)) => 12
    let arena = new_arena();
    let env = default_env(&arena);

    let plus = sym(&arena, "+");
    let mul = sym(&arena, "*");
    let minus = sym(&arena, "-");
    let two = arena.alloc(Value::Int(2)).unwrap();
    let three = arena.alloc(Value::Int(3)).unwrap();
    let ten = arena.alloc(Value::Int(10)).unwrap();
    let four = arena.alloc(Value::Int(4)).unwrap();

    let mul_expr = list(&arena, &[mul, two, three]);
    let sub_expr = list(&arena, &[minus, ten, four]);
    let expr = list(&arena, &[plus, mul_expr, sub_expr]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 12);
}

// ============================================================================
// Comparisons
// ============================================================================

#[test]
fn test_eq() {
    let arena = new_arena();
    let env = default_env(&arena);

    let eq = sym(&arena, "=");
    let a = arena.alloc(Value::Int(5)).unwrap();
    let b = arena.alloc(Value::Int(5)).unwrap();
    let expr = list(&arena, &[eq, a, b]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_bool(&arena, result), true);
}

#[test]
fn test_lt() {
    let arena = new_arena();
    let env = default_env(&arena);

    let lt = sym(&arena, "<");
    let a = arena.alloc(Value::Int(3)).unwrap();
    let b = arena.alloc(Value::Int(5)).unwrap();
    let expr = list(&arena, &[lt, a, b]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_bool(&arena, result), true);
}

// ============================================================================
// Quote
// ============================================================================

#[test]
fn test_quote() {
    // (quote 42) => 42
    let arena = new_arena();
    let env = default_env(&arena);

    let quote = sym(&arena, "quote");
    let val = arena.alloc(Value::Int(42)).unwrap();
    let expr = list(&arena, &[quote, val]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 42);
}

// ============================================================================
// If
// ============================================================================

#[test]
fn test_if_true() {
    // (if #t 1 2) => 1
    let arena = new_arena();
    let env = default_env(&arena);

    let if_sym = sym(&arena, "if");
    let cond = arena.alloc(Value::Bool(true)).unwrap();
    let then_val = arena.alloc(Value::Int(1)).unwrap();
    let else_val = arena.alloc(Value::Int(2)).unwrap();
    let expr = list(&arena, &[if_sym, cond, then_val, else_val]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 1);
}

#[test]
fn test_if_false() {
    // (if #f 1 2) => 2
    let arena = new_arena();
    let env = default_env(&arena);

    let if_sym = sym(&arena, "if");
    let cond = arena.alloc(Value::Bool(false)).unwrap();
    let then_val = arena.alloc(Value::Int(1)).unwrap();
    let else_val = arena.alloc(Value::Int(2)).unwrap();
    let expr = list(&arena, &[if_sym, cond, then_val, else_val]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 2);
}

#[test]
fn test_if_with_expression_condition() {
    // (if (= 1 1) 10 20) => 10
    let arena = new_arena();
    let env = default_env(&arena);

    let if_sym = sym(&arena, "if");
    let eq = sym(&arena, "=");
    let one_a = arena.alloc(Value::Int(1)).unwrap();
    let one_b = arena.alloc(Value::Int(1)).unwrap();
    let cond = list(&arena, &[eq, one_a, one_b]);
    let then_val = arena.alloc(Value::Int(10)).unwrap();
    let else_val = arena.alloc(Value::Int(20)).unwrap();
    let expr = list(&arena, &[if_sym, cond, then_val, else_val]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 10);
}

// ============================================================================
// fn (lambda)
// ============================================================================

#[test]
fn test_fn_application() {
    // ((fn (x) (+ x 1)) 5) => 6
    let arena = new_arena();
    let env = default_env(&arena);

    let fn_sym = sym(&arena, "fn");
    let x = sym(&arena, "x");
    let plus = sym(&arena, "+");
    let one = arena.alloc(Value::Int(1)).unwrap();
    let body = list(&arena, &[plus, x, one]);
    let params = list(&arena, &[x]);
    let lambda = list(&arena, &[fn_sym, params, body]);

    let five = arena.alloc(Value::Int(5)).unwrap();
    let expr = list(&arena, &[lambda, five]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 6);
}

#[test]
fn test_fn_closure() {
    // (let ((a 10)) ((fn (x) (+ x a)) 5)) => 15
    let arena = new_arena();
    let env = default_env(&arena);

    let let_sym = sym(&arena, "let");
    let fn_sym = sym(&arena, "fn");
    let a = sym(&arena, "a");
    let x = sym(&arena, "x");
    let plus = sym(&arena, "+");
    let ten = arena.alloc(Value::Int(10)).unwrap();
    let five = arena.alloc(Value::Int(5)).unwrap();

    let body = list(&arena, &[plus, x, a]);
    let params = list(&arena, &[x]);
    let lambda = list(&arena, &[fn_sym, params, body]);
    let call = list(&arena, &[lambda, five]);

    let binding = list(&arena, &[a, ten]);
    let bindings = list(&arena, &[binding]);
    let expr = list(&arena, &[let_sym, bindings, call]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 15);
}

// ============================================================================
// def
// ============================================================================

#[test]
fn test_def_simple() {
    // (def x 42)
    // x => 42
    let arena = new_arena();
    let env = default_env(&arena);

    let def = sym(&arena, "def");
    let x = sym(&arena, "x");
    let val = arena.alloc(Value::Int(42)).unwrap();
    let def_expr = list(&arena, &[def, x, val]);

    let x_ref = sym(&arena, "x");
    let program = list(&arena, &[def_expr, x_ref]);

    let (result, _) = eval_sequence(&arena, program, env).unwrap();
    assert_eq!(get_int(&arena, result), 42);
}

#[test]
fn test_def_function() {
    // (def (add1 n) (+ n 1))
    // (add1 5) => 6
    let arena = new_arena();
    let env = default_env(&arena);

    let def = sym(&arena, "def");
    let add1 = sym(&arena, "add1");
    let n = sym(&arena, "n");
    let plus = sym(&arena, "+");
    let one = arena.alloc(Value::Int(1)).unwrap();
    let body = list(&arena, &[plus, n, one]);
    let name_params = list(&arena, &[add1, n]);
    let def_expr = list(&arena, &[def, name_params, body]);

    let add1_ref = sym(&arena, "add1");
    let five = arena.alloc(Value::Int(5)).unwrap();
    let call = list(&arena, &[add1_ref, five]);

    let program = list(&arena, &[def_expr, call]);

    let (result, _) = eval_sequence(&arena, program, env).unwrap();
    assert_eq!(get_int(&arena, result), 6);
}

// ============================================================================
// let
// ============================================================================

#[test]
fn test_let_simple() {
    // (let ((x 5)) x) => 5
    let arena = new_arena();
    let env = default_env(&arena);

    let let_sym = sym(&arena, "let");
    let x = sym(&arena, "x");
    let five = arena.alloc(Value::Int(5)).unwrap();

    let binding = list(&arena, &[x, five]);
    let bindings = list(&arena, &[binding]);
    let body = sym(&arena, "x");
    let expr = list(&arena, &[let_sym, bindings, body]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 5);
}

#[test]
fn test_let_multiple_bindings() {
    // (let ((x 3) (y 4)) (+ x y)) => 7
    let arena = new_arena();
    let env = default_env(&arena);

    let let_sym = sym(&arena, "let");
    let x = sym(&arena, "x");
    let y = sym(&arena, "y");
    let plus = sym(&arena, "+");
    let three = arena.alloc(Value::Int(3)).unwrap();
    let four = arena.alloc(Value::Int(4)).unwrap();

    let bx = list(&arena, &[x, three]);
    let by = list(&arena, &[y, four]);
    let bindings = list(&arena, &[bx, by]);
    let body = list(&arena, &[plus, sym(&arena, "x"), sym(&arena, "y")]);
    let expr = list(&arena, &[let_sym, bindings, body]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 7);
}

// ============================================================================
// do
// ============================================================================

#[test]
fn test_do_returns_last() {
    // (do 1 2 3) => 3
    let arena = new_arena();
    let env = default_env(&arena);

    let do_sym = sym(&arena, "do");
    let one = arena.alloc(Value::Int(1)).unwrap();
    let two = arena.alloc(Value::Int(2)).unwrap();
    let three = arena.alloc(Value::Int(3)).unwrap();
    let expr = list(&arena, &[do_sym, one, two, three]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 3);
}

// ============================================================================
// cons / car / cdr / null? / pair?
// ============================================================================

#[test]
fn test_cons_car_cdr() {
    // (car (cons 1 2)) => 1
    let arena = new_arena();
    let env = default_env(&arena);

    let cons = sym(&arena, "cons");
    let car = sym(&arena, "car");
    let one = arena.alloc(Value::Int(1)).unwrap();
    let two = arena.alloc(Value::Int(2)).unwrap();
    let cons_expr = list(&arena, &[cons, one, two]);
    let expr = list(&arena, &[car, cons_expr]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 1);
}

#[test]
fn test_cdr() {
    // (cdr (cons 1 2)) => 2
    let arena = new_arena();
    let env = default_env(&arena);

    let cons = sym(&arena, "cons");
    let cdr = sym(&arena, "cdr");
    let one = arena.alloc(Value::Int(1)).unwrap();
    let two = arena.alloc(Value::Int(2)).unwrap();
    let cons_expr = list(&arena, &[cons, one, two]);
    let expr = list(&arena, &[cdr, cons_expr]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 2);
}

#[test]
fn test_null_predicate() {
    let arena = new_arena();
    let env = default_env(&arena);

    // (null? (quote ())) => #t
    let null_p = sym(&arena, "null?");
    let quote = sym(&arena, "quote");
    let nil = arena.alloc(Value::Nil).unwrap();
    let quoted_nil = list(&arena, &[quote, nil]);
    let expr = list(&arena, &[null_p, quoted_nil]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_bool(&arena, result), true);
}

#[test]
fn test_pair_predicate() {
    let arena = new_arena();
    let env = default_env(&arena);

    // (pair? (cons 1 2)) => #t
    let pair_p = sym(&arena, "pair?");
    let cons = sym(&arena, "cons");
    let one = arena.alloc(Value::Int(1)).unwrap();
    let two = arena.alloc(Value::Int(2)).unwrap();
    let cons_expr = list(&arena, &[cons, one, two]);
    let expr = list(&arena, &[pair_p, cons_expr]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_bool(&arena, result), true);
}

// ============================================================================
// Laziness — pure functions should not evaluate unused args
// ============================================================================

#[test]
fn test_laziness_unused_arg() {
    // (def (first a b) a)
    // The second argument should never be evaluated (it's a thunk).
    // We can't easily test "not evaluated" without IO, but we can
    // confirm the result is correct.
    //
    // (let ((x 1)) ((fn (a b) a) x (/ 1 0)))
    // If lazy, this should return 1 without evaluating (/ 1 0).
    let arena = new_arena();
    let env = default_env(&arena);

    let fn_sym = sym(&arena, "fn");
    let a = sym(&arena, "a");
    let b = sym(&arena, "b");
    let params = list(&arena, &[a, b]);
    let body = sym(&arena, "a");
    let lambda = list(&arena, &[fn_sym, params, body]);

    let one = arena.alloc(Value::Int(1)).unwrap();
    let div = sym(&arena, "/");
    let one2 = arena.alloc(Value::Int(1)).unwrap();
    let zero = arena.alloc(Value::Int(0)).unwrap();
    let div_by_zero = list(&arena, &[div, one2, zero]);

    let expr = list(&arena, &[lambda, one, div_by_zero]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 1);
}

// ============================================================================
// Tail call — should not stack overflow
// ============================================================================

#[test]
fn test_tail_call_no_overflow() {
    // (def (loop n) (if (= n 0) 0 (loop (- n 1))))
    // (loop 10000) => 0
    let arena = new_arena();
    let env = default_env(&arena);

    let def = sym(&arena, "def");
    let loop_sym = sym(&arena, "loop");
    let n = sym(&arena, "n");
    let if_sym = sym(&arena, "if");
    let eq = sym(&arena, "=");
    let minus = sym(&arena, "-");

    let n_ref = sym(&arena, "n");
    let zero = arena.alloc(Value::Int(0)).unwrap();
    let one = arena.alloc(Value::Int(1)).unwrap();

    let cond = list(&arena, &[eq, n_ref, zero]);
    let result_val = arena.alloc(Value::Int(0)).unwrap();

    let loop_ref = sym(&arena, "loop");
    let n_ref2 = sym(&arena, "n");
    let decr = list(&arena, &[minus, n_ref2, one]);
    let recurse = list(&arena, &[loop_ref, decr]);

    let body = list(&arena, &[if_sym, cond, result_val, recurse]);
    let name_params = list(&arena, &[loop_sym, n]);
    let def_expr = list(&arena, &[def, name_params, body]);

    let loop_call = sym(&arena, "loop");
    let big_n = arena.alloc(Value::Int(1000)).unwrap();
    let call = list(&arena, &[loop_call, big_n]);

    let program = list(&arena, &[def_expr, call]);

    let (result, _) = eval_sequence(&arena, program, env).unwrap();
    assert_eq!(get_int(&arena, result), 0);
}

// ============================================================================
// Cons list operations
// ============================================================================

#[test]
fn test_cons_does_not_force() {
    // cons should not force its arguments
    // (cons (/ 1 0) 2) should succeed (thunk not forced)
    // Then (cdr (cons (/ 1 0) 2)) => 2
    let arena = new_arena();
    let env = default_env(&arena);

    let fn_sym = sym(&arena, "fn");
    let a = sym(&arena, "a");
    let b = sym(&arena, "b");

    // Create a pure function that calls cons
    let cons_sym = sym(&arena, "cons");
    let a_ref = sym(&arena, "a");
    let b_ref = sym(&arena, "b");
    let cons_call = list(&arena, &[cons_sym, a_ref, b_ref]);
    let params = list(&arena, &[a, b]);
    let lambda = list(&arena, &[fn_sym, params, cons_call]);

    let div = sym(&arena, "/");
    let one = arena.alloc(Value::Int(1)).unwrap();
    let zero = arena.alloc(Value::Int(0)).unwrap();
    let div_by_zero = list(&arena, &[div, one, zero]);
    let two = arena.alloc(Value::Int(2)).unwrap();

    let call = list(&arena, &[lambda, div_by_zero, two]);
    let cdr_sym = sym(&arena, "cdr");
    let expr = list(&arena, &[cdr_sym, call]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 2);
}

// ============================================================================
// Match
// ============================================================================

#[test]
fn test_match_int() {
    // (match 1
    //   (0 10)
    //   (1 20)
    //   (_ 30))
    // => 20
    let arena = new_arena();
    let env = default_env(&arena);

    let match_sym = sym(&arena, "match");
    let one = arena.alloc(Value::Int(1)).unwrap();

    let zero = arena.alloc(Value::Int(0)).unwrap();
    let ten = arena.alloc(Value::Int(10)).unwrap();
    let clause0 = list(&arena, &[zero, ten]);

    let one2 = arena.alloc(Value::Int(1)).unwrap();
    let twenty = arena.alloc(Value::Int(20)).unwrap();
    let clause1 = list(&arena, &[one2, twenty]);

    let wild = sym(&arena, "_");
    let thirty = arena.alloc(Value::Int(30)).unwrap();
    let clause_w = list(&arena, &[wild, thirty]);

    let expr = list(&arena, &[match_sym, one, clause0, clause1, clause_w]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 20);
}

#[test]
fn test_match_wildcard() {
    // (match 99
    //   (_ 42))
    // => 42
    let arena = new_arena();
    let env = default_env(&arena);

    let match_sym = sym(&arena, "match");
    let val = arena.alloc(Value::Int(99)).unwrap();
    let wild = sym(&arena, "_");
    let result_val = arena.alloc(Value::Int(42)).unwrap();
    let clause = list(&arena, &[wild, result_val]);
    let expr = list(&arena, &[match_sym, val, clause]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 42);
}

#[test]
fn test_match_variable_binding() {
    // (match 5
    //   (x (+ x 10)))
    // => 15
    let arena = new_arena();
    let env = default_env(&arena);

    let match_sym = sym(&arena, "match");
    let five = arena.alloc(Value::Int(5)).unwrap();
    let x = sym(&arena, "x");
    let plus = sym(&arena, "+");
    let x_ref = sym(&arena, "x");
    let ten = arena.alloc(Value::Int(10)).unwrap();
    let body = list(&arena, &[plus, x_ref, ten]);
    let clause = list(&arena, &[x, body]);
    let expr = list(&arena, &[match_sym, five, clause]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_int(&arena, result), 15);
}

// ============================================================================
// not
// ============================================================================

#[test]
fn test_not() {
    let arena = new_arena();
    let env = default_env(&arena);

    let not_sym = sym(&arena, "not");
    let t = arena.alloc(Value::Bool(true)).unwrap();
    let expr = list(&arena, &[not_sym, t]);

    let result = run(&arena, expr, env).unwrap();
    assert_eq!(get_bool(&arena, result), false);
}

// ============================================================================
// Unbound variable error
// ============================================================================

#[test]
fn test_unbound_variable() {
    let arena = new_arena();
    let env = default_env(&arena);

    let x = sym(&arena, "nonexistent_var");
    let result = run(&arena, x, env);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind, ErrorKind::UnboundVariable);
}

// ============================================================================
// Thunk memoization
// ============================================================================

#[test]
fn test_thunk_memoization() {
    // (def x (+ 1 2))
    // (+ x x) => 6
    // Both uses of x should return 3
    let arena = new_arena();
    let env = default_env(&arena);

    let def = sym(&arena, "def");
    let x = sym(&arena, "x");
    let plus = sym(&arena, "+");
    let one = arena.alloc(Value::Int(1)).unwrap();
    let two = arena.alloc(Value::Int(2)).unwrap();
    let add_expr = list(&arena, &[plus, one, two]);
    let def_expr = list(&arena, &[def, x, add_expr]);

    let x1 = sym(&arena, "x");
    let x2 = sym(&arena, "x");
    let plus2 = sym(&arena, "+");
    let use_expr = list(&arena, &[plus2, x1, x2]);

    let program = list(&arena, &[def_expr, use_expr]);

    let (result, _) = eval_sequence(&arena, program, env).unwrap();
    assert_eq!(get_int(&arena, result), 6);
}

// ============================================================================
// Infinite list (laziness via cons)
// ============================================================================

#[test]
fn test_infinite_list_car() {
    // (def (ones) (cons 1 (ones)))
    // (car (ones)) => 1
    let arena = new_arena();
    let env = default_env(&arena);

    let def = sym(&arena, "def");
    let ones = sym(&arena, "ones");
    let cons_sym = sym(&arena, "cons");
    let car_sym = sym(&arena, "car");
    let one = arena.alloc(Value::Int(1)).unwrap();
    let ones_ref = sym(&arena, "ones");
    let ones_call = list(&arena, &[ones_ref]);
    let cons_call = list(&arena, &[cons_sym, one, ones_call]);

    let name_params = list(&arena, &[ones]);
    let def_expr = list(&arena, &[def, name_params, cons_call]);

    let ones_ref2 = sym(&arena, "ones");
    let call_ones = list(&arena, &[ones_ref2]);
    let car_call = list(&arena, &[car_sym, call_ones]);

    let program = list(&arena, &[def_expr, car_call]);

    let (result, _) = eval_sequence(&arena, program, env).unwrap();
    assert_eq!(get_int(&arena, result), 1);
}
