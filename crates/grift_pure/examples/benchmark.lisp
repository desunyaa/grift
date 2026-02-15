;;; benchmark.lisp — Timed benchmark suite for grift_pure
;;;
;;; This file uses grift_pure's Rust-inspired Lisp dialect:
;;;   fn    — lambda / function literal  (like Rust's `fn`)
;;;   def   — define a binding           (like Rust's `let` at top level)
;;;   let   — local bindings             (like Rust's `let` block)
;;;   match — pattern matching           (like Rust's `match`)
;;;   do    — sequential evaluation      (like Rust's block `{ }`)
;;;   if    — conditional                (like Rust's `if`)
;;;
;;; Types are conceptual (the interpreter is dynamically typed):
;;;   i64   — integer values
;;;   bool  — #t / #f
;;;   cons  — pair / linked list cell
;;;   nil   — empty list / unit
;;;
;;; Run with: cargo run -p grift_pure --example bench

;;; =========================================================================
;;; Benchmark 1: Fibonacci (recursive, exponential)
;;; Tests: function calls, recursion depth, arithmetic
;;; =========================================================================

;; (def (fib n) -> i64
;;   "Naive recursive fibonacci. O(2^n) time complexity."
;;   (if (< n 2)
;;       n
;;       (+ (fib (- n 1))
;;          (fib (- n 2)))))

;; Targets:
;;   fib(10)  =        55   — ~1K   calls, instant
;;   fib(20)  =      6765   — ~1M   calls, measurable
;;   fib(30)  =    832040   — ~1B   calls, seconds
;;   fib(40)  = 102334155   — ~1T   calls, push the limits

;;; =========================================================================
;;; Benchmark 2: Tail-recursive countdown
;;; Tests: tail call optimization, loop performance
;;; =========================================================================

;; (def (countdown n) -> i64
;;   "Tail-recursive countdown to 0. Tests TCO."
;;   (if (= n 0)
;;       0
;;       (countdown (- n 1))))

;; Targets:
;;   countdown(10000)   — tests basic TCO
;;   countdown(100000)  — stress test TCO

;;; =========================================================================
;;; Benchmark 3: Ackermann function
;;; Tests: deeply nested recursion, stack depth
;;; =========================================================================

;; (def (ack m n) -> i64
;;   "Ackermann function. Grows extremely fast."
;;   (if (= m 0)
;;       (+ n 1)
;;       (if (= n 0)
;;           (ack (- m 1) 1)
;;           (ack (- m 1) (ack m (- n 1))))))

;; Targets:
;;   ack(2, 3)  =    9
;;   ack(3, 3)  =   61
;;   ack(3, 6)  =  509

;;; =========================================================================
;;; Benchmark 4: Sum of list
;;; Tests: cons/car/cdr, list traversal, pair operations
;;; =========================================================================

;; (def (sum-range n) -> i64
;;   "Sum integers from 1 to n using accumulator pattern."
;;   (let ((go (fn (i acc)
;;               (if (= i 0)
;;                   acc
;;                   (go (- i 1) (+ acc i))))))
;;     (go n 0)))

;; Targets:
;;   sum-range(100)   =  5050
;;   sum-range(1000)  = 500500

;;; =========================================================================
;;; Benchmark 5: Higher-order function composition
;;; Tests: closures, first-class functions, fn as values
;;; =========================================================================

;; (def (compose f g)
;;   "Returns (fn (x) (f (g x)))"
;;   (fn (x) (f (g x))))
;;
;; (let ((add1   (fn (x) (+ x 1)))
;;       (double (fn (x) (* x 2))))
;;   ((compose add1 double) 5))   ;; => 11

;;; =========================================================================
;;; Benchmark 6: Church numerals
;;; Tests: lambda calculus encoding, abstraction overhead
;;; =========================================================================

;; (def (church n)
;;   "Encode i64 as Church numeral (fn (f x) ...)."
;;   (if (= n 0)
;;       (fn (f x) x)
;;       (let ((pred (church (- n 1))))
;;         (fn (f x) (f (pred f x))))))
;;
;; (def (unchurch c)
;;   "Decode Church numeral back to i64."
;;   (c (fn (x) (+ x 1)) 0))
;;
;; Targets:
;;   (unchurch (church 10)) = 10

;;; =========================================================================
;;; Benchmark 7: Pattern matching stress
;;; Tests: match dispatch, wildcard patterns, variable binding
;;; =========================================================================

;; (def (classify n) -> str
;;   "Classify an integer using match."
;;   (match n
;;     (0 0)
;;     (1 1)
;;     (_ (if (< n 0) (- 0 n) n))))
;;
;; Targets:
;;   (classify 0)   = 0
;;   (classify 1)   = 1
;;   (classify -5)  = 5
;;   (classify 42)  = 42
