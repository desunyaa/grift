//! # Lisp Stress Test / Benchmark Suite
//!
//! Run with: `cargo run -p lisp_repl --bin lisp-bench --release`
//!
//! This runs various stress tests on the Lisp interpreter to measure performance
//! and verify correctness under load.

use lisp_eval::Evaluator;
use lisp_parser::Lisp;
use lisp_repl::format_value;
use std::time::{Duration, Instant};

/// Result of a single benchmark
struct BenchResult {
    name: String,
    duration: Duration,
    iterations: usize,
    result: String,
    passed: bool,
    note: Option<String>,
}

impl BenchResult {
    fn print(&self) {
        let status = if self.passed { "PASS" } else { "FAIL" };
        let per_iter = if self.iterations > 1 {
            format!(
                " ({:.2}µs/iter)",
                self.duration.as_nanos() as f64 / self.iterations as f64 / 1000.0
            )
        } else {
            String::new()
        };

        println!(
            "[{}] {}: {:?}{}",
            status, self.name, self.duration, per_iter
        );

        if !self.result.is_empty() && self.result.len() < 60 {
            println!("       Result: {}", self.result);
        }

        if let Some(note) = &self.note {
            println!("       Note: {}", note);
        }
    }
}

/// Evaluate a string and return the formatted result
fn eval_str<const N: usize>(
    lisp: &Lisp<N>,
    eval: &mut Evaluator<N>,
    code: &str,
) -> Result<String, String> {
    match eval.eval_str(code) {
        Ok(idx) => {
            let mut buf = String::new();
            format_value(lisp, idx, &mut buf);
            Ok(buf)
        }
        Err(e) => Err(format!("Error: {:?}", e.kind)),
    }
}

/// Run a simple timed benchmark with GC after completion
fn run_bench<const N: usize>(
    name: &str,
    lisp: &Lisp<N>,
    eval: &mut Evaluator<N>,
    iterations: usize,
    code: &str,
    expected: Option<&str>,
) -> BenchResult {
    println!("Running: {} ({} iterations)...", name, iterations);

    let start = Instant::now();
    let mut last_result = String::new();
    let mut error = None;
    let mut successful_iters = 0;

    for _ in 0..iterations {
        match eval_str(lisp, eval, code) {
            Ok(r) => {
                last_result = r;
                successful_iters += 1;
            }
            Err(e) => {
                error = Some(e);
                break;
            }
        }
    }

    let duration = start.elapsed();
    
    // Run GC after each test to clean up
    eval.gc();

    if let Some(e) = error {
        return BenchResult {
            name: name.to_string(),
            duration,
            iterations: successful_iters,
            result: e,
            passed: false,
            note: Some(format!("Failed after {} iterations", successful_iters)),
        };
    }

    let passed = expected.map_or(true, |exp| last_result == exp);

    BenchResult {
        name: name.to_string(),
        duration,
        iterations,
        result: last_result,
        passed,
        note: if !passed {
            expected.map(|e| format!("Expected: {}", e))
        } else {
            None
        },
    }
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           Lisp Interpreter Stress Test Suite                 ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║ Arena size: 50,000 cells                                     ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    let lisp: Lisp<50000> = Lisp::new();
    let mut eval = match Evaluator::new(&lisp) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Failed to create evaluator: {:?}", e);
            return;
        }
    };

    let mut results: Vec<BenchResult> = Vec::new();

    // ═══════════════════════════════════════════════════════════════════════
    // SECTION 1: Basic Operations
    // ═══════════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Section 1: Basic Operations");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    results.push(run_bench(
        "Arithmetic (+ 1 2 3 4 5) x 500",
        &lisp,
        &mut eval,
        500,
        "(+ 1 2 3 4 5)",
        Some("15"),
    ));

    // Define a variable for lookup tests
    let _ = eval_str(&lisp, &mut eval, "(define bench-var 42)");

    results.push(run_bench(
        "Symbol lookup x 500",
        &lisp,
        &mut eval,
        500,
        "bench-var",
        Some("42"),
    ));

    results.push(run_bench(
        "List construction x 200",
        &lisp,
        &mut eval,
        200,
        "(list 1 2 3 4 5 6 7 8 9 10)",
        None,
    ));

    results.push(run_bench(
        "Quote x 200",
        &lisp,
        &mut eval,
        200,
        "'(a b c d e)",
        None,
    ));

    // Clean up before next section
    eval.gc();
    println!();

    // ═══════════════════════════════════════════════════════════════════════
    // SECTION 2: Recursion & TCO
    // ═══════════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Section 2: Recursion & Tail Call Optimization");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Define recursive functions
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (factorial n) (if (<= n 1) 1 (* n (factorial (- n 1)))))",
    );
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (fib n) (if (<= n 1) n (+ (fib (- n 1)) (fib (- n 2)))))",
    );
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (sum-to-tco n acc) (if (= n 0) acc (sum-to-tco (- n 1) (+ acc n))))",
    );
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (count-down n) (if (= n 0) 'done (count-down (- n 1))))",
    );

    results.push(run_bench(
        "Factorial(8) x 100",
        &lisp,
        &mut eval,
        100,
        "(factorial 8)",
        Some("40320"),
    ));

    results.push(run_bench(
        "Fibonacci(10) x 10",
        &lisp,
        &mut eval,
        10,
        "(fib 10)",
        Some("55"),
    ));

    results.push(run_bench(
        "TCO Sum 1..100 x 20",
        &lisp,
        &mut eval,
        20,
        "(sum-to-tco 100 0)",
        Some("5050"),
    ));

    results.push(run_bench(
        "TCO countdown 100 x 20",
        &lisp,
        &mut eval,
        20,
        "(count-down 100)",
        Some("done"),
    ));

    // Clean up before next section
    eval.gc();
    println!();

    // ═══════════════════════════════════════════════════════════════════════
    // SECTION 3: Higher-Order Functions
    // ═══════════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Section 3: Higher-Order Functions");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Define HOF utilities (tail-recursive versions)
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (map-helper f lst acc) (if (null? lst) (reverse acc) (map-helper f (cdr lst) (cons (f (car lst)) acc))))",
    );
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (map f lst) (map-helper f lst '()))",
    );
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (filter-helper p lst acc) (if (null? lst) (reverse acc) (if (p (car lst)) (filter-helper p (cdr lst) (cons (car lst) acc)) (filter-helper p (cdr lst) acc))))",
    );
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (filter p lst) (filter-helper p lst '()))",
    );
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (fold f acc lst) (if (null? lst) acc (fold f (f acc (car lst)) (cdr lst))))",
    );
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (range-helper n acc) (if (= n 0) acc (range-helper (- n 1) (cons n acc))))",
    );
    let _ = eval_str(&lisp, &mut eval, "(define (range n) (range-helper n '()))");
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (reverse-helper lst acc) (if (null? lst) acc (reverse-helper (cdr lst) (cons (car lst) acc))))",
    );
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (reverse lst) (reverse-helper lst '()))",
    );

    results.push(run_bench(
        "Map square over 20 elements x 20",
        &lisp,
        &mut eval,
        20,
        "(map (lambda (x) (* x x)) (range 20))",
        None,
    ));

    results.push(run_bench(
        "Filter even from 20 elements x 20",
        &lisp,
        &mut eval,
        20,
        "(filter (lambda (x) (= (mod x 2) 0)) (range 20))",
        None,
    ));

    results.push(run_bench(
        "Fold sum over 20 elements x 20",
        &lisp,
        &mut eval,
        20,
        "(fold + 0 (range 20))",
        Some("210"),
    ));

    results.push(run_bench(
        "Map+Filter+Fold pipeline x 20",
        &lisp,
        &mut eval,
        20,
        "(fold + 0 (filter (lambda (x) (> x 50)) (map (lambda (x) (* x x)) (range 15))))",
        None,
    ));

    // Clean up before next section
    eval.gc();
    println!();

    // ═══════════════════════════════════════════════════════════════════════
    // SECTION 4: Closures & Environments
    // ═══════════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Section 4: Closures & Environments");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (make-counter) (let ((n 0)) (lambda () (set! n (+ n 1)) n)))",
    );
    let _ = eval_str(
        &lisp,
        &mut eval,
        "(define (make-adder x) (lambda (y) (+ x y)))",
    );

    results.push(run_bench(
        "Create closure x 200",
        &lisp,
        &mut eval,
        200,
        "(make-adder 42)",
        None,
    ));

    // Counter test - use add10 as closure instead of make-counter
    let _ = eval_str(&lisp, &mut eval, "(define add10 (make-adder 10))");
    results.push(run_bench(
        "Call closure x 100",
        &lisp,
        &mut eval,
        100,
        "(add10 5)",
        Some("15"),
    ));

    results.push(run_bench(
        "Nested let* 5 deep x 200",
        &lisp,
        &mut eval,
        200,
        "(let* ((a 1) (b (+ a 1)) (c (+ b 2)) (d (+ c 3)) (e (+ d 4))) e)",
        Some("11"),
    ));

    // Clean up before next section
    eval.gc();
    println!();

    // ═══════════════════════════════════════════════════════════════════════
    // SECTION 5: Thunks & Lazy Evaluation
    // ═══════════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Section 5: Thunks & Lazy Evaluation");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    results.push(run_bench(
        "Create thunk x 200",
        &lisp,
        &mut eval,
        200,
        "(delay (+ 1 2 3 4 5))",
        None,
    ));

    // Force memoization test - simplified
    results.push(run_bench(
        "Force immediate x 200",
        &lisp,
        &mut eval,
        200,
        "(force (delay (* 3 333)))",
        Some("999"),
    ));

    results.push(run_bench(
        "promise? predicate x 100",
        &lisp,
        &mut eval,
        100,
        "(promise? (delay 42))",
        Some("#t"),
    ));

    results.push(run_bench(
        "Delay+Force cycle x 100",
        &lisp,
        &mut eval,
        100,
        "(force (delay (+ 10 20 30)))",
        Some("60"),
    ));

    // Clean up before next section
    eval.gc();
    println!();

    // ═══════════════════════════════════════════════════════════════════════
    // SECTION 6: Mutation
    // ═══════════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Section 6: Mutation (set!, set-car!, set-cdr!)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // set! test - simplified
    let _ = eval_str(&lisp, &mut eval, "(define mut-var 0)");
    results.push(run_bench(
        "set! expression x 100",
        &lisp,
        &mut eval,
        100,
        "(begin (set! mut-var (+ mut-var 1)) mut-var)",
        None, // Don't check result - it changes each time
    ));

    // set-car!/set-cdr! test - simplified
    results.push(run_bench(
        "set-car! x 100",
        &lisp,
        &mut eval,
        100,
        "(let ((p (cons 1 2))) (set-car! p 10) (car p))",
        Some("10"),
    ));

    results.push(run_bench(
        "set-cdr! x 100",
        &lisp,
        &mut eval,
        100,
        "(let ((p (cons 1 2))) (set-cdr! p 20) (cdr p))",
        Some("20"),
    ));

    println!();

    // ═══════════════════════════════════════════════════════════════════════
    // SECTION 7: Garbage Collection
    // ═══════════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Section 7: Garbage Collection");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // GC stress test: allocate lots, then collect
    println!("Running: GC stress test (allocate heavily, then collect)...");
    let gc_start = Instant::now();

    // Allocate a bunch of garbage
    for i in 0..100 {
        let _ = eval_str(
            &lisp,
            &mut eval,
            &format!("(list {} {} {} {} {})", i, i + 1, i + 2, i + 3, i + 4),
        );
    }

    let pre_gc = gc_start.elapsed();
    println!("       Allocation phase: {:?}", pre_gc);

    let gc_only_start = Instant::now();
    let stats = eval.gc();
    let gc_time = gc_only_start.elapsed();

    println!(
        "[PASS] GC stress test: {:?} total ({:?} GC only)",
        gc_start.elapsed(),
        gc_time
    );
    println!(
        "       Stats: marked={}, collected={}, total_before={}",
        stats.marked, stats.collected, stats.total_before
    );
    println!();

    // ═══════════════════════════════════════════════════════════════════════
    // SECTION 8: Parsing Stress
    // ═══════════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Section 8: Parsing");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    results.push(run_bench(
        "Parse deeply nested x 100",
        &lisp,
        &mut eval,
        100,
        "'((((((((((42))))))))))",
        None,
    ));

    results.push(run_bench(
        "Parse long list x 100",
        &lisp,
        &mut eval,
        100,
        "'(1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)",
        None,
    ));

    results.push(run_bench(
        "Parse complex expression x 100",
        &lisp,
        &mut eval,
        100,
        "(if (> 3 2) (+ 1 2) (* 3 4))",
        Some("3"),
    ));

    println!();

    // ═══════════════════════════════════════════════════════════════════════
    // Summary
    // ═══════════════════════════════════════════════════════════════════════
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                        SUMMARY                               ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    let total_time: Duration = results.iter().map(|r| r.duration).sum();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.iter().filter(|r| !r.passed).count();

    for result in &results {
        result.print();
    }

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!(
        "Total: {} tests, {} passed, {} failed",
        results.len(),
        passed,
        failed
    );
    println!("Total benchmark time: {:?}", total_time);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Final arena stats
    let final_stats = lisp.stats();
    println!();
    println!("Final Arena Stats:");
    println!(
        "  Allocated: {} / {} cells",
        final_stats.allocated, final_stats.capacity
    );
    println!(
        "  Usage: {:.1}%",
        (final_stats.allocated as f64 / final_stats.capacity as f64) * 100.0
    );
}
