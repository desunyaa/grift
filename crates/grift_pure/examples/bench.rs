//! Timed benchmark runner for grift_pure.
//!
//! Builds ASTs manually and times evaluation of several benchmark programs
//! using the CPS trampoline evaluator. Reports execution times to measure
//! interpreter performance limits.
//!
//! Run with: `cargo run -p grift_pure --example bench`
//!
//! See `examples/benchmark.lisp` for the Lisp source descriptions.

use grift_pure::*;
use grift_pure::eval::{run, eval_sequence};
use grift_pure::intern::intern;
use std::time::Instant;
use std::panic;

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

/// Extract an integer from a value, following Evaluated redirects.
fn get_int(arena: &Arena<Value, ARENA_SIZE>, idx: ArenaIndex) -> i64 {
    let mut cur = idx;
    loop {
        match arena.get(cur).unwrap() {
            Value::Int(n) => return n,
            Value::Evaluated { value } => cur = value,
            _ => panic!("expected Int"),
        }
    }
}

// ============================================================================
// Benchmark: Fibonacci
// ============================================================================

/// Build: (def (fib n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2)))))
///        (fib <input>)
fn build_fib_program(arena: &Arena<Value, ARENA_SIZE>, input: i64) -> ArenaIndex {
    let def = sym(arena, "def");
    let fib = sym(arena, "fib");
    let n = sym(arena, "n");
    let if_sym = sym(arena, "if");
    let lt = sym(arena, "<");
    let plus = sym(arena, "+");
    let minus = sym(arena, "-");

    let n_ref1 = sym(arena, "n");
    let two = arena.alloc(Value::Int(2)).unwrap();
    let cond = list(arena, &[lt, n_ref1, two]);

    let n_ref2 = sym(arena, "n");
    let one1 = arena.alloc(Value::Int(1)).unwrap();
    let n_minus_1 = list(arena, &[minus, n_ref2, one1]);

    let n_ref3 = sym(arena, "n");
    let two2 = arena.alloc(Value::Int(2)).unwrap();
    let n_minus_2 = list(arena, &[minus, n_ref3, two2]);

    let fib_ref1 = sym(arena, "fib");
    let fib_n1 = list(arena, &[fib_ref1, n_minus_1]);

    let fib_ref2 = sym(arena, "fib");
    let fib_n2 = list(arena, &[fib_ref2, n_minus_2]);

    let sum = list(arena, &[plus, fib_n1, fib_n2]);
    let n_ref4 = sym(arena, "n");
    let body = list(arena, &[if_sym, cond, n_ref4, sum]);

    let name_params = list(arena, &[fib, n]);
    let def_expr = list(arena, &[def, name_params, body]);

    let fib_call = sym(arena, "fib");
    let input_val = arena.alloc(Value::Int(input)).unwrap();
    let call = list(arena, &[fib_call, input_val]);

    list(arena, &[def_expr, call])
}

// ============================================================================
// Benchmark: Tail-recursive countdown
// ============================================================================

/// Build: (def (countdown n) (if (= n 0) 0 (countdown (- n 1))))
///        (countdown <input>)
fn build_countdown_program(arena: &Arena<Value, ARENA_SIZE>, input: i64) -> ArenaIndex {
    let def = sym(arena, "def");
    let countdown = sym(arena, "countdown");
    let n = sym(arena, "n");
    let if_sym = sym(arena, "if");
    let eq = sym(arena, "=");
    let minus = sym(arena, "-");

    let n_ref1 = sym(arena, "n");
    let zero = arena.alloc(Value::Int(0)).unwrap();
    let cond = list(arena, &[eq, n_ref1, zero]);

    let result_val = arena.alloc(Value::Int(0)).unwrap();

    let countdown_ref = sym(arena, "countdown");
    let n_ref2 = sym(arena, "n");
    let one = arena.alloc(Value::Int(1)).unwrap();
    let decr = list(arena, &[minus, n_ref2, one]);
    let recurse = list(arena, &[countdown_ref, decr]);

    let body = list(arena, &[if_sym, cond, result_val, recurse]);
    let name_params = list(arena, &[countdown, n]);
    let def_expr = list(arena, &[def, name_params, body]);

    let call_sym = sym(arena, "countdown");
    let input_val = arena.alloc(Value::Int(input)).unwrap();
    let call = list(arena, &[call_sym, input_val]);

    list(arena, &[def_expr, call])
}

// ============================================================================
// Benchmark: Ackermann function
// ============================================================================

/// Build: (def (ack m n) (if (= m 0) (+ n 1)
///                            (if (= n 0) (ack (- m 1) 1)
///                                (ack (- m 1) (ack m (- n 1))))))
///        (ack <m> <n>)
fn build_ack_program(arena: &Arena<Value, ARENA_SIZE>, m: i64, n: i64) -> ArenaIndex {
    let def = sym(arena, "def");
    let ack = sym(arena, "ack");
    let m_sym = sym(arena, "m");
    let n_sym = sym(arena, "n");
    let if_sym = sym(arena, "if");
    let eq = sym(arena, "=");
    let plus = sym(arena, "+");
    let minus = sym(arena, "-");

    // (= m 0)
    let m_ref1 = sym(arena, "m");
    let zero1 = arena.alloc(Value::Int(0)).unwrap();
    let cond1 = list(arena, &[eq, m_ref1, zero1]);

    // (+ n 1)
    let n_ref1 = sym(arena, "n");
    let one1 = arena.alloc(Value::Int(1)).unwrap();
    let base = list(arena, &[plus, n_ref1, one1]);

    // (= n 0)
    let n_ref2 = sym(arena, "n");
    let zero2 = arena.alloc(Value::Int(0)).unwrap();
    let cond2 = list(arena, &[eq, n_ref2, zero2]);

    // (ack (- m 1) 1)
    let ack_ref1 = sym(arena, "ack");
    let m_ref2 = sym(arena, "m");
    let one2 = arena.alloc(Value::Int(1)).unwrap();
    let m_dec = list(arena, &[minus, m_ref2, one2]);
    let one3 = arena.alloc(Value::Int(1)).unwrap();
    let case2 = list(arena, &[ack_ref1, m_dec, one3]);

    // (- n 1)
    let n_ref3 = sym(arena, "n");
    let one4 = arena.alloc(Value::Int(1)).unwrap();
    let n_dec = list(arena, &[minus, n_ref3, one4]);

    // (ack m (- n 1))
    let ack_ref2 = sym(arena, "ack");
    let m_ref3 = sym(arena, "m");
    let inner = list(arena, &[ack_ref2, m_ref3, n_dec]);

    // (ack (- m 1) (ack m (- n 1)))
    let ack_ref3 = sym(arena, "ack");
    let m_ref4 = sym(arena, "m");
    let one5 = arena.alloc(Value::Int(1)).unwrap();
    let m_dec2 = list(arena, &[minus, m_ref4, one5]);
    let case3 = list(arena, &[ack_ref3, m_dec2, inner]);

    // (if (= n 0) (ack (- m 1) 1) (ack (- m 1) (ack m (- n 1))))
    let inner_if = list(arena, &[if_sym, cond2, case2, case3]);

    // (if (= m 0) (+ n 1) <inner_if>)
    let body = list(arena, &[if_sym, cond1, base, inner_if]);

    let name_params = list(arena, &[ack, m_sym, n_sym]);
    let def_expr = list(arena, &[def, name_params, body]);

    let call_sym = sym(arena, "ack");
    let m_val = arena.alloc(Value::Int(m)).unwrap();
    let n_val = arena.alloc(Value::Int(n)).unwrap();
    let call = list(arena, &[call_sym, m_val, n_val]);

    list(arena, &[def_expr, call])
}

// ============================================================================
// Benchmark: Higher-order composition
// ============================================================================

/// Build: (let ((add1 (fn (x) (+ x 1)))
///              (double (fn (x) (* x 2))))
///           (add1 (double <input>)))
fn build_compose_program(arena: &Arena<Value, ARENA_SIZE>, input: i64) -> ArenaIndex {
    let let_sym = sym(arena, "let");
    let fn_sym = sym(arena, "fn");
    let plus = sym(arena, "+");
    let mul = sym(arena, "*");

    // add1 = (fn (x) (+ x 1))
    let add1 = sym(arena, "add1");
    let x1 = sym(arena, "x");
    let x1_ref = sym(arena, "x");
    let one = arena.alloc(Value::Int(1)).unwrap();
    let add1_body = list(arena, &[plus, x1_ref, one]);
    let add1_params = list(arena, &[x1]);
    let add1_fn = list(arena, &[fn_sym, add1_params, add1_body]);

    // double = (fn (x) (* x 2))
    let double = sym(arena, "double");
    let x2 = sym(arena, "x");
    let x2_ref = sym(arena, "x");
    let two = arena.alloc(Value::Int(2)).unwrap();
    let double_body = list(arena, &[mul, x2_ref, two]);
    let double_params = list(arena, &[x2]);
    let double_fn = list(arena, &[fn_sym, double_params, double_body]);

    let b_add1 = list(arena, &[add1, add1_fn]);
    let b_double = list(arena, &[double, double_fn]);
    let bindings = list(arena, &[b_add1, b_double]);

    // (add1 (double <input>))
    let double_ref = sym(arena, "double");
    let input_val = arena.alloc(Value::Int(input)).unwrap();
    let double_call = list(arena, &[double_ref, input_val]);
    let add1_ref = sym(arena, "add1");
    let body = list(arena, &[add1_ref, double_call]);

    list(arena, &[let_sym, bindings, body])
}

// ============================================================================
// Benchmark: Pattern matching
// ============================================================================

/// Build: (match <input> (0 0) (1 1) (x (+ x x)))
fn build_match_program(arena: &Arena<Value, ARENA_SIZE>, input: i64) -> ArenaIndex {
    let match_sym = sym(arena, "match");
    let plus = sym(arena, "+");

    let val = arena.alloc(Value::Int(input)).unwrap();

    let zero = arena.alloc(Value::Int(0)).unwrap();
    let zero2 = arena.alloc(Value::Int(0)).unwrap();
    let clause0 = list(arena, &[zero, zero2]);

    let one = arena.alloc(Value::Int(1)).unwrap();
    let one2 = arena.alloc(Value::Int(1)).unwrap();
    let clause1 = list(arena, &[one, one2]);

    // Variable pattern: x binds the matched value
    let x = sym(arena, "x");
    let x_ref1 = sym(arena, "x");
    let x_ref2 = sym(arena, "x");
    let var_body = list(arena, &[plus, x_ref1, x_ref2]);
    let clause_v = list(arena, &[x, var_body]);

    list(arena, &[match_sym, val, clause0, clause1, clause_v])
}

// ============================================================================
// Runner
// ============================================================================

/// Run a single benchmark, printing timing results.
/// Catches panics from arena OOM gracefully.
fn run_bench(name: &str, expected: i64, f: impl FnOnce() -> Result<ArenaIndex, EvalError>, arena: &Arena<Value, ARENA_SIZE>) {
    let start = Instant::now();
    let result = panic::catch_unwind(panic::AssertUnwindSafe(f));
    let elapsed = start.elapsed();
    match result {
        Ok(Ok(idx)) => {
            let actual = get_int(arena, idx);
            let status = if actual == expected { "✓" } else { "✗ MISMATCH" };
            println!("  {status} {name:<40} = {actual:<15} ({elapsed:>12.3?})  expected {expected}");
        }
        Ok(Err(e)) => {
            println!("  ✗ {name:<40}   ERROR: {:?}  ({elapsed:>12.3?})", e.kind);
        }
        Err(_) => {
            println!("  ✗ {name:<40}   OOM (arena exhausted)  ({elapsed:>12.3?})");
        }
    }
}

fn main() {
    println!("═══════════════════════════════════════════════════════════════════");
    println!("  grift_pure benchmark suite");
    println!("  Arena size: {} slots", ARENA_SIZE);
    println!("  Keywords: fn, def, let, match, do, if (Rust-inspired)");
    println!("  See examples/benchmark.lisp for Lisp source descriptions");
    println!("═══════════════════════════════════════════════════════════════════");
    println!();

    // ---- Fibonacci ----
    println!("── Benchmark 1: Fibonacci (recursive, O(2^n)) ──");
    for &n in &[0i64, 1, 5, 10, 12] {
        let arena = new_arena();
        let env = default_env(&arena);
        let program = build_fib_program(&arena, n);
        let expected = naive_fib(n);
        run_bench(
            &format!("fib({})", n),
            expected,
            || eval_sequence(&arena, program, env).map(|(r, _)| r),
            &arena,
        );
    }
    println!();

    // ---- Fibonacci: push the limits ----
    println!("── Benchmark 1b: Fibonacci — pushing arena limits ──");
    println!("  (fib(40) = 102334155 — requires ~2^40 calls,");
    println!("   far beyond a 65536-slot arena. Testing max feasible n...)");
    for &n in &[13i64, 14, 15, 20] {
        let arena = new_arena();
        let env = default_env(&arena);
        let program = build_fib_program(&arena, n);
        let expected = naive_fib(n);
        run_bench(
            &format!("fib({})", n),
            expected,
            || eval_sequence(&arena, program, env).map(|(r, _)| r),
            &arena,
        );
    }
    println!();

    // ---- Countdown (tail call) ----
    println!("── Benchmark 2: Tail-recursive countdown ──");
    for &n in &[100i64, 1000, 5000] {
        let arena = new_arena();
        let env = default_env(&arena);
        let program = build_countdown_program(&arena, n);
        run_bench(
            &format!("countdown({})", n),
            0,
            || eval_sequence(&arena, program, env).map(|(r, _)| r),
            &arena,
        );
    }
    println!();

    // ---- Ackermann ----
    println!("── Benchmark 3: Ackermann function ──");
    for &(m, n) in &[(2, 3), (3, 3), (3, 4)] {
        let arena = new_arena();
        let env = default_env(&arena);
        let program = build_ack_program(&arena, m, n);
        let expected = naive_ack(m, n);
        run_bench(
            &format!("ack({}, {})", m, n),
            expected,
            || eval_sequence(&arena, program, env).map(|(r, _)| r),
            &arena,
        );
    }
    println!();

    // ---- Higher-order composition ----
    println!("── Benchmark 4: Higher-order function composition ──");
    for &input in &[5i64, 100, 1000] {
        let arena = new_arena();
        let env = default_env(&arena);
        let program = build_compose_program(&arena, input);
        let expected = input * 2 + 1;
        run_bench(
            &format!("compose(add1, double)({})", input),
            expected,
            || run(&arena, program, env),
            &arena,
        );
    }
    println!();

    // ---- Pattern matching ----
    println!("── Benchmark 5: Pattern matching ──");
    for &input in &[0i64, 1, 42] {
        let arena = new_arena();
        let env = default_env(&arena);
        let program = build_match_program(&arena, input);
        let expected = match input { 0 => 0, 1 => 1, n => n + n };
        run_bench(
            &format!("match({})", input),
            expected,
            || run(&arena, program, env),
            &arena,
        );
    }
    println!();

    println!("═══════════════════════════════════════════════════════════════════");
    println!("  Note: fib(40) = 102334155 requires ~2^40 recursive calls.");
    println!("  A tree-walking interpreter with a fixed 65536-slot arena");
    println!("  cannot compute fib(40) — it runs out of arena memory around");
    println!("  fib(12). This demonstrates the practical limits of pure");
    println!("  arena-based evaluation without garbage collection during");
    println!("  recursive descent.");
    println!("═══════════════════════════════════════════════════════════════════");
}

/// Native iterative fibonacci for computing expected values.
fn naive_fib(n: i64) -> i64 {
    if n < 2 { return n; }
    let (mut a, mut b) = (0i64, 1i64);
    for _ in 2..=n {
        let c = a + b;
        a = b;
        b = c;
    }
    b
}

/// Native Rust Ackermann for computing expected values.
fn naive_ack(m: i64, n: i64) -> i64 {
    if m == 0 {
        n + 1
    } else if n == 0 {
        naive_ack(m - 1, 1)
    } else {
        naive_ack(m - 1, naive_ack(m, n - 1))
    }
}
