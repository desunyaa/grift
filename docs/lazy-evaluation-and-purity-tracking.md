# Lazy Evaluation and Purity Tracking in Grift

## Abstract

Grift is a `no_std`, `no_alloc` R7RS Scheme implementation built on a
fixed-size arena allocator. This document describes Grift's lazy evaluation
strategy — call-by-need with memoization — and its relationship to the
foundational literature on lazy evaluation. We then present the design of a
**purity tracker**: a dynamic analysis that classifies functions as *pure* or
*impure* at runtime, enabling Grift to lazily evaluate pure expressions while
eagerly evaluating impure ones (I/O, mutation, and any function that
transitively depends on an impure operation). This hybrid strategy preserves
the semantic benefits of laziness for referentially transparent code while
guaranteeing correct ordering of side effects.

---

## 1. Background: Lazy Evaluation in the Literature

Lazy evaluation — also known as *call-by-need* — delays the evaluation of an
expression until its value is actually required, and then caches (memoizes) the
result so that subsequent accesses are O(1). The theoretical and practical
foundations of this strategy have been developed across several landmark papers.

### 1.1 A Natural Semantics for Lazy Evaluation

Launchbury (1993) formalized lazy evaluation with a **big-step natural
semantics** that operates over a *heap* of bindings. In Launchbury's semantics,
a **configuration** is a pair ⟨Heap, Expression⟩, and the judgment

> Γ : e ⇓ Δ : v

reads: "In heap Γ, expression e evaluates to value v, producing updated
heap Δ." The heap is threaded through evaluation to model memoization — once a
binding is forced, its heap entry is updated from a *closure* (unevaluated
expression plus environment) to its *value* (weak head normal form).

Grift's evaluation directly mirrors this semantics:

| Launchbury Concept    | Grift Implementation                            |
|-----------------------|--------------------------------------------------|
| Heap binding          | Arena slot containing a `Value`                  |
| Closure (unevaluated) | `Value::Thunk { expr, env }`                     |
| Value (WHNF)          | `Value::Nil`, `Number`, `Lambda`, `Cons`, etc.   |
| Heap update           | Overwrite arena slot with `Value::Indirection(_)` |
| Configuration ⟨Γ, e⟩ | `(ArenaIndex, ArenaIndex)` pair in `eval()`      |

The correctness of Grift's evaluator can be verified by establishing a
bisimulation between its force/eval loop and Launchbury's big-step rules.

### 1.2 A Call-by-Need Lambda Calculus

Ariola, Felleisen, Maraist, Odersky, and Wadler (1995) provided the
**equational theory** underlying call-by-need: a lambda calculus in which
reduction is demand-driven and bindings are shared. Their calculus introduces
*answer contexts* and *evaluation contexts* that formalize when and where
reduction occurs.

Grift realizes this equational theory operationally. When `eval()` encounters a
variable reference, it returns the associated thunk *without forcing it*. Only
when the value is actually consumed — for instance, as an argument to a strict
built-in like `+` — does `force()` trigger evaluation. This demand-driven
strategy is exactly the operational content of the call-by-need calculus.

### 1.3 The Call-by-Need Lambda Calculus, Revisited

Chang and Felleisen (2012) simplified the 1995 calculus to a single axiom,
**β_need**, which combines substitution and demand into one step. A key insight
is that once a thunk's value has been consumed, the original thunk cell can be
*discarded* — or more precisely, replaced with a direct pointer to the result.

This is precisely what Grift does when it updates a forced thunk to
`Value::Indirection(result)`. The thunk-to-indirection update is the
operational realization of β_need: the original suspended computation is
replaced by its value, and all future references follow the indirection in
constant time.

### 1.4 Deriving a Lazy Abstract Machine

Sestoft (1997) systematically derived **small-step abstract machines** from
Launchbury's big-step semantics. Sestoft's machines separate evaluation into
two phases:

1. **Eval** — Decompose an expression into sub-expressions that need forcing.
2. **Force** — Drive a sub-expression to weak head normal form.

Grift's evaluator mirrors this two-phase architecture:

- **`eval()`** is the main evaluation loop. It pattern-matches on the
  expression, handles special forms and function application, and implements
  tail-call optimization by looping rather than recursing.
- **`force()`** is a dedicated function that takes a potentially unevaluated
  value (thunk or indirection) and drives it to WHNF. It implements the
  black-hole protocol, memoization, and indirection chasing.

This force/eval split is not accidental — it directly corresponds to the
machine structure Sestoft derived from first principles.

### 1.5 Tail Recursion Without Space Leaks

Jones (1992) demonstrated that **black-holing** — marking a thunk as "under
evaluation" before forcing it — is essential for space-safe tail recursion.
Without black-holing, chains of tail-recursive calls can retain references to
already-evaluated thunks, causing unbounded space growth.

Grift implements black-holing via `Value::BlackHole`. When `force()` begins
evaluating a thunk, it immediately overwrites the arena slot with `BlackHole`
before proceeding. This serves two purposes:

1. **Cycle detection** — If evaluation of the thunk transitively attempts to
   force itself, the evaluator encounters `BlackHole` and reports an error
   (infinite loop detected).
2. **Space safety** — The overwrite releases the reference to the original
   thunk expression and environment, allowing the garbage collector to reclaim
   them if they are unreachable.

### 1.6 Improving the Lazy Krivine Machine

Friedman, Ghuloum, Siek, and Winebarger (2007) described two key
optimizations for lazy machines:

- **Indirection short-circuiting** — When following an indirection chain,
  collapse intermediate indirections so that future accesses go directly to the
  final value.
- **Update-marker optimization** — Avoid redundant updates by tracking whether
  a thunk has already been updated.

Grift's `force()` function implements indirection chasing in a loop: it follows
`Value::Indirection(_)` pointers iteratively until it reaches a non-indirection
value, which is the operational equivalent of short-circuiting. The arena's
cell-based design means that updates are in-place overwrites, and the
indirection-to-WHNF path is always short (typically one hop after the first
force).

### 1.7 The Spineless Tagless G-machine (STG)

Peyton Jones (1992) described the **STG machine**, the production-quality
abstract machine underlying GHC. Key design elements relevant to Grift
include:

- **Closures as heap objects** — In STG, every heap object is a closure with an
  entry code pointer and free variables. Grift's `Value::Thunk { expr, env }`
  is the arena-allocated analogue: the `expr` is the "code" and `env` captures
  the free variables.
- **Update frames** — STG uses update frames on the stack to record where to
  write the result after forcing a thunk. Grift uses in-place arena slot
  overwrites instead, which is simpler but achieves the same memoization
  effect.
- **Tagging** — STG uses tags to distinguish evaluated values from unevaluated
  thunks. Grift's `Value` enum provides this discrimination via Rust's enum
  discriminant.

While Grift does not implement the full STG machine (it uses a tree-walking
evaluator rather than compiled code), the structural parallels are clear and
intentional. A future compilation backend could target an STG-like
representation while reusing the same arena and value model.

### 1.8 Push/Enter vs. Eval/Apply

Marlow and Peyton Jones (2006) compared two strategies for higher-order
function application:

- **Push/Enter** — Push all arguments onto the stack and enter the function,
  which consumes as many as it needs.
- **Eval/Apply** — Evaluate the function to WHNF first, then apply arguments
  based on the function's arity.

Grift uses an **Eval/Apply** strategy: `eval()` first evaluates the operator
position of a function application to WHNF, determines whether the result is a
`Lambda` or `Builtin`, and then applies arguments accordingly. For lambdas,
arguments are bound lazily (as thunks) in the function's environment. For
builtins, arguments are forced to WHNF before the built-in handler is invoked.

This choice is natural for a tree-walking interpreter where function arity is
dynamically determined, and it avoids the complexity of stack-based argument
passing.

### 1.9 Implementing Functional Languages: A Tutorial

Peyton Jones and Lester (1992) provided a step-by-step textbook treatment of
multiple lazy abstract machines, including template instantiation, the
G-machine, and the TIM (Three Instruction Machine). Grift's evaluator most
closely resembles the **template instantiation** approach: expressions are
arena-allocated "templates" that are instantiated (evaluated) on demand, with
results cached via indirections.

---

## 2. Grift's Current Lazy Evaluation Architecture

### 2.1 Value Representation

All values in Grift live in a fixed-size arena of `Cell<Value>` slots.
The `Value` enum includes five categories relevant to lazy evaluation:

```
Value::Thunk { expr, env }   — Unevaluated: expression + captured environment
Value::BlackHole             — Under evaluation (cycle guard)
Value::Indirection(idx)      — Evaluated: pointer to WHNF result
Value::Lambda { params, .. } — WHNF: a closure
Value::Nil | Number | ...    — WHNF: self-evaluating literals
```

### 2.2 The Force Protocol

When `force()` is called on an `ArenaIndex`:

```
force(idx):
  loop:
    match arena[idx]:
      WHNF value       → return idx           // already evaluated
      Indirection(tgt) → idx = tgt; continue   // chase pointer
      BlackHole        → error: circular ref   // cycle detected
      Thunk{expr, env} →
        arena[idx] = BlackHole                  // (1) black-hole
        gc_roots.push(idx)                      // (2) protect from GC
        result = eval(expr, env)                // (3) evaluate
        if is_black_hole(result) → error        // (4) check cycle
        arena[idx] = Indirection(result)        // (5) memoize
        gc_roots.pop(idx)                       // (6) unprotect
        return result
```

### 2.3 Argument Binding: Lazy vs. Strict

Grift uses two distinct argument-binding strategies:

| Context        | Strategy   | Function             | Behavior                          |
|----------------|------------|----------------------|-----------------------------------|
| Lambda calls   | **Lazy**   | `bind_args_lazy()`   | Wrap each argument in a `Thunk`   |
| Built-in calls | **Strict** | `force_args()`       | Force each argument to WHNF       |

For lambda applications, `bind_args_lazy()` creates a thunk for each argument
expression, capturing the *caller's* environment. The thunk is bound to the
corresponding parameter name in the *function's* environment. The argument is
never evaluated unless and until the function body references it.

For built-in functions (arithmetic, comparisons, list operations, type
predicates), `force_args()` evaluates all arguments to WHNF before invoking the
built-in handler. This is necessary because built-in handlers operate on
concrete values, not thunks.

### 2.4 Tail-Call Optimization

The `eval()` function uses a loop-based trampoline for tail-call optimization.
When a lambda is applied in tail position:

1. Bind arguments lazily in the function's environment.
2. Update `expr` and `env` to the function body and new environment.
3. **Continue the loop** (rather than making a recursive call).

This ensures that tail-recursive programs execute in constant stack space,
consistent with R7RS's requirement for proper tail calls.

---

## 3. The Problem: Laziness and Side Effects

Lazy evaluation and side effects are fundamentally in tension. Laziness delays
and reorders evaluation, which is safe for pure (referentially transparent)
expressions but can change program semantics when effects are involved.

Consider:

```scheme
(define (greet name)
  (display "Hello, ")
  (display name)
  (newline))

(define greeting (greet "World"))
```

Under lazy evaluation, if the call `(greet "World")` is wrapped in a thunk and
never forced, the `display` calls never execute. Worse, if forced at an
unexpected time, the output may appear out of order relative to other I/O.

The standard solution in purely functional languages (Haskell) is to
sequence effects through monads. But Scheme is an *impure* language — side
effects are first-class and unrestricted. Grift therefore needs a different
approach: **selectively apply laziness only where it is safe**.

---

## 4. Design: Dynamic Purity Tracking

We propose a **purity tracker** — a dynamic analysis integrated into Grift's
evaluator — that classifies every function and expression as either *pure* or
*impure* at runtime. The tracker enforces a simple invariant:

> **Pure expressions are evaluated lazily (call-by-need).**
> **Impure expressions are evaluated eagerly (call-by-value).**

This hybrid strategy is *semantically conservative*: it defaults to eager
evaluation for anything that might have effects, and only applies laziness when
it can prove safety.

### 4.1 Definitions

- **Pure function**: A function whose result depends only on its arguments and
  which produces no observable side effects (no I/O, no mutation, no
  continuation capture with visible effects).
- **Impure function**: A function that performs I/O, mutates state (via `set!`,
  `set-car!`, `set-cdr!`, `string-set!`, etc.), or calls any impure function.
- **Purity propagation**: A function that *calls* an impure function is itself
  impure. Purity is *not* transitive upward — calling a pure function does not
  make the caller pure if the caller performs other impure operations.

### 4.2 Purity Classification

The tracker classifies operations into three categories:

#### 4.2.1 Intrinsically Impure Operations

These are the *leaves* of the impurity lattice — operations that are impure by
definition:

| Category      | Operations                                                    |
|---------------|---------------------------------------------------------------|
| Output I/O    | `display`, `write`, `newline`, `write-char`, `write-string`   |
| Input I/O     | `read`, `read-char`, `read-line`, `read-string`               |
| Port I/O      | `open-input-file`, `open-output-file`, `close-port`, etc.     |
| Mutation      | `set!`, `set-car!`, `set-cdr!`, `string-set!`, `vector-set!` |
| Continuations | `call/cc` (when the continuation escapes)                     |
| Environment   | `eval`, `load`, `interaction-environment`                     |
| System        | `exit`, `error`, `raise`                                      |

#### 4.2.2 Intrinsically Pure Operations

These operations are always pure:

| Category      | Operations                                                    |
|---------------|---------------------------------------------------------------|
| Arithmetic    | `+`, `-`, `*`, `/`, `modulo`, `remainder`, `abs`, `gcd`, etc. |
| Comparison    | `=`, `<`, `>`, `<=`, `>=`, `eq?`, `eqv?`, `equal?`           |
| Type checks   | `null?`, `pair?`, `number?`, `symbol?`, `string?`, etc.       |
| List access   | `car`, `cdr`, `list-ref`, `length`, `append`, `map`, `filter` |
| Constructors  | `cons`, `list`, `vector`, `make-string`                       |
| String access | `string-ref`, `string-length`, `substring`, `string-append`  |
| Math          | `floor`, `ceiling`, `truncate`, `round`, `sqrt`, `expt`      |

#### 4.2.3 Derived Purity (User-Defined Functions)

User-defined functions (lambdas) have their purity **inferred** at runtime by
tracking what operations they invoke:

```
purity(lambda) =
  if body references any impure operation → Impure
  if body calls any function known to be impure → Impure
  otherwise → Pure
```

### 4.3 Representation in the Arena

The purity tracker must fit within Grift's `no_std`, `no_alloc` constraints.
We extend the `Value` enum and the evaluation machinery with minimal changes.

#### 4.3.1 Purity Bit on Lambdas

Add a purity flag to the `Lambda` variant:

```rust
enum Purity {
    Pure,       // Known pure — safe to defer
    Impure,     // Known impure — must evaluate eagerly
    Unknown,    // Not yet determined — defaults to eager (conservative)
}

enum Value {
    // ... existing variants ...
    Lambda {
        params: ArenaIndex,
        body_env: ArenaIndex,
        purity: Purity,       // NEW: purity classification
    },
    // ...
}
```

`Purity` is a `Copy` type occupying a single byte, maintaining the requirement
that all arena-stored values implement `Copy`.

#### 4.3.2 Impure Built-in Registry

Built-in functions are identified by a `u8` index. We partition this index
space into pure and impure regions:

```rust
impl Evaluator {
    fn is_builtin_pure(id: u8) -> bool {
        // Pure builtins occupy indices 0..PURE_BUILTIN_COUNT
        // Impure builtins occupy indices PURE_BUILTIN_COUNT..
        id < PURE_BUILTIN_COUNT
    }
}
```

Alternatively, a compact bitmap (a `u128` or a `[u8; 32]`) can encode the
purity of all 256 possible built-in slots, using one bit per built-in. This
avoids any ordering constraint on the ID space:

```rust
const IMPURE_BUILTINS: u128 = {
    (1 << BUILTIN_DISPLAY)
    | (1 << BUILTIN_WRITE)
    | (1 << BUILTIN_NEWLINE)
    | (1 << BUILTIN_READ)
    | (1 << BUILTIN_SET_CAR)
    | (1 << BUILTIN_SET_CDR)
    // ... additional impure builtins
};

fn is_builtin_pure(id: u8) -> bool {
    (IMPURE_BUILTINS & (1u128 << id)) == 0
}
```

### 4.4 Purity Inference Algorithm

Purity is inferred **dynamically** during evaluation, not statically before
execution. This is a deliberate design choice: static purity analysis in a
language with first-class functions, `eval`, and `call/cc` is undecidable in
general. Dynamic tracking sidesteps this by observing actual behavior.

#### 4.4.1 Algorithm Overview

```
infer_purity(lambda_body, env):
  taint = Pure
  for each form in lambda_body:
    match form:
      // Direct impure operation
      (set! ...)        → taint = Impure
      (display ...)     → taint = Impure

      // Function call
      (f args...)       →
        f_val = eval(f, env)
        match f_val:
          Builtin(id)   → if !is_builtin_pure(id): taint = Impure
          Lambda{purity} →
            match purity:
              Impure   → taint = Impure
              Unknown  → taint = Impure  // conservative
              Pure     → (no change)

      // Nested lambda (does not taint outer if not called)
      (lambda ...)      → (no change to outer taint)

      // Other forms
      _                 → (recurse into sub-expressions)

  return taint
```

#### 4.4.2 Handling `Unknown` Purity

When a lambda is first created, its purity is `Unknown`. Upon first
invocation, the evaluator performs purity inference on the body:

1. Walk the body's AST, checking each operation and function call.
2. If any impure operation or impure callee is found, mark as `Impure`.
3. If the walk completes without finding impurity, mark as `Pure`.
4. Cache the result in the `Lambda` value's `purity` field (in-place arena
   update).

Subsequent calls skip inference and use the cached purity. If the function
is called with different closures that could affect purity (e.g., a
higher-order function receiving an impure callback), the inference is
conservative: calling any `Unknown` or `Impure` function taints the caller.

#### 4.4.3 Propagation Rule

The key correctness property is **upward propagation of impurity**:

> If function `f` calls function `g`, and `g` is impure, then `f` is impure.

This is transitive: if `g` calls `h` and `h` is impure, then `g` is impure,
and therefore `f` is impure. The inference algorithm ensures this by checking
callee purity at each call site.

Note that purity does *not* propagate downward. If a pure function `f` is
called by an impure function `g`, `f` remains pure. The impurity of `g` does
not affect `f`'s classification.

### 4.5 Modified Evaluation Rules

With the purity tracker in place, the evaluator's behavior changes at two
points:

#### 4.5.1 Argument Binding

```
apply(f, args, call_env):
  match f.purity:
    Pure →
      // Call-by-need: wrap args in thunks
      bind_args_lazy(f.env, f.params, args, call_env)

    Impure | Unknown →
      // Call-by-value: force all args before binding
      forced_args = force_args(args, call_env)
      bind_args_strict(f.env, f.params, forced_args)
```

This ensures that:
- **Pure functions** benefit from laziness: unused arguments are never
  evaluated, and shared arguments are evaluated at most once.
- **Impure functions** get predictable left-to-right argument evaluation,
  matching R7RS's unspecified-but-conventional evaluation order.

#### 4.5.2 Let-Binding

```
eval_let(bindings, body, env):
  for (name, expr) in bindings:
    purity = infer_expr_purity(expr, env)
    match purity:
      Pure    → env = extend(env, name, Thunk{expr, env})
      Impure  → val = eval(expr, env); env = extend(env, name, val)
  eval(body, env)
```

#### 4.5.3 Cons and List Construction

Pure `cons` and `list` expressions continue to create thunks for their
elements (lazy construction). If the expression involves impure computations,
elements are forced before cons cell creation.

### 4.6 Interaction with Existing Mechanisms

#### 4.6.1 Black-Hole Protocol

The black-hole protocol is orthogonal to purity tracking. Black-holing
applies only to thunks, and impure expressions are never thunked under the
purity tracker. Therefore, black-holes only arise for pure thunks, where
cycle detection remains necessary (e.g., `(define x x)` is a pure but
circular definition).

#### 4.6.2 Garbage Collection

The purity flag is a small, `Copy`-able field within the `Value` enum. It
does not introduce new `ArenaIndex` references, so it has no impact on the
mark-and-sweep GC's tracing logic. The `Trace` implementation for `Value`
remains unchanged.

#### 4.6.3 Tail-Call Optimization

TCO continues to work as before. The only change is that the decision to
trampoline (continue the loop) vs. return is augmented with a purity check
at function application time. For an impure tail call, arguments are forced
before the trampoline updates `expr` and `env`.

#### 4.6.4 Continuations and `call/cc`

`call/cc` is classified as impure because captured continuations can observe
and alter control flow in ways that interact with evaluation order. Any
function invoking `call/cc` is therefore eagerly evaluated, ensuring that
continuation captures occur at well-defined points.

### 4.7 Correctness Argument

The purity tracker's correctness rests on a single invariant:

> **If an expression is classified as Pure, then its evaluation is
> referentially transparent: it produces the same result regardless of when
> it is evaluated, and it produces no observable side effects.**

Given this invariant, lazy evaluation of pure expressions is semantics-
preserving: the result is the same whether the expression is evaluated
immediately or deferred and memoized.

For impure expressions, eager evaluation preserves the standard Scheme
semantics: effects occur in program order (specifically, in the order that
the evaluator encounters them during its left-to-right, top-to-bottom
traversal).

The conservative default — treating `Unknown` as `Impure` — ensures that the
system never incorrectly delays an effectful computation. The worst case is
that a pure function is eagerly evaluated (a performance loss, not a
correctness violation).

**Relation to Launchbury (1993):** Launchbury's semantics assumes a pure
language. Our purity tracker recovers this assumption for the pure subset of
a Scheme program, allowing Launchbury's heap semantics to apply within that
subset.

**Relation to Ariola et al. (1995) and Chang & Felleisen (2012):** The
call-by-need calculus and its β_need axiom are valid only for pure terms. The
purity tracker ensures that β_need is applied only where it is sound, and
that impure terms are reduced under standard call-by-value rules.

**Relation to Jones (1992):** Black-holing for space safety applies only to
thunks, which exist only for pure expressions under the tracker. Space leaks
from unevaluated impure thunks are eliminated by construction.

### 4.8 Example Walkthrough

Consider the following program:

```scheme
(define (square x) (* x x))        ; pure: only uses *
(define (add a b) (+ a b))         ; pure: only uses +

(define (greet name)                ; impure: uses display, newline
  (display "Hello, ")
  (display name)
  (newline))

(define (compute x)                 ; impure: calls greet (impure)
  (greet "World")
  (square x))

(define result (compute 42))
```

**Purity inference:**

1. `square` → walks body `(* x x)`: `*` is a pure builtin → **Pure**.
2. `add` → walks body `(+ a b)`: `+` is a pure builtin → **Pure**.
3. `greet` → walks body: `display` is an impure builtin → **Impure**.
4. `compute` → walks body: calls `greet` (impure) → **Impure**.

**Evaluation of `(compute 42)`:**

1. `compute` is **Impure** → argument `42` is **forced eagerly**.
2. `bind_args_strict(compute.env, (x), (42))` → `x = 42` (not a thunk).
3. Evaluate body: `(greet "World")` → `greet` is impure, args forced,
   `display` executes immediately. Output: `Hello, World\n`.
4. `(square x)` → `square` is **Pure**. Under lazy evaluation, `x` would be
   thunked, but here `x` is already a value (42) since the caller was impure.
   `square` computes `(* 42 42)` → `1764`.
5. `result` is bound to `1764`.

If instead we had:

```scheme
(define result (square (add 3 4)))
```

Both `square` and `add` are pure, so:

1. `add 3 4` is wrapped in a thunk (not evaluated yet).
2. `square` receives the thunk as `x`.
3. When `(* x x)` is evaluated, `x` is forced *once* (evaluating `(add 3 4)`
   → `7`), memoized, and the second reference to `x` sees the cached `7`.
4. Result: `49`.

---

## 5. Implementation Roadmap

### Phase 1: Built-in Purity Registry

- Annotate each built-in function as pure or impure via a constant bitmap.
- No behavioral change yet — this is a data-only addition.

### Phase 2: Lambda Purity Field

- Add the `Purity` enum and the `purity` field to `Value::Lambda`.
- Initialize all user-defined lambdas with `Purity::Unknown`.
- Update `Trace`, `Copy`, `Clone`, `PartialEq` implementations.

### Phase 3: Purity Inference

- Implement `infer_purity()` as a walk over the lambda body AST.
- Call it on first invocation of each lambda.
- Cache the result in the arena.

### Phase 4: Modified Argument Binding

- Branch in `eval()` at function application: lazy for pure, strict for
  impure.
- Update `let`, `cons`, and other binding forms similarly.

### Phase 5: Testing and Validation

- **Pure function tests**: Verify that pure functions are lazily evaluated
  (unused arguments not evaluated, shared arguments evaluated once).
- **Impure function tests**: Verify that I/O occurs in correct order.
- **Propagation tests**: Verify that calling an impure function taints the
  caller.
- **Edge cases**: `call/cc`, higher-order functions with mixed-purity
  callbacks, recursive functions.

---

## 6. Future Work

- **Static purity analysis**: A conservative static analysis pass before
  evaluation could pre-classify many functions, reducing the cost of runtime
  inference.
- **Purity annotations**: Allow programmers to declare `(pure (lambda ...))` to
  skip inference.
- **Selective strictness annotations**: An `(eager expr)` form to force
  immediate evaluation regardless of purity.
- **Effect types**: A richer effect system distinguishing I/O, mutation, and
  exceptions.
- **Benchmark suite**: Measure the performance impact of the purity tracker on
  real Scheme programs.

---

## 7. References

1. Launchbury, J. (1993). "A Natural Semantics for Lazy Evaluation."
   *Proceedings of the 20th ACM SIGPLAN-SIGACT Symposium on Principles of
   Programming Languages (POPL '93)*, pp. 144–154. ACM.

2. Ariola, Z.M., Felleisen, M., Maraist, J., Odersky, M. & Wadler, P.
   (1995). "A Call-by-Need Lambda Calculus." *Proceedings of the 22nd ACM
   SIGPLAN-SIGACT Symposium on Principles of Programming Languages
   (POPL '95)*, pp. 233–246. ACM.

3. Chang, S. & Felleisen, M. (2012). "The Call-by-Need Lambda Calculus,
   Revisited." *European Symposium on Programming (ESOP 2012)*, Lecture Notes
   in Computer Science, vol. 7211, pp. 128–147. Springer.

4. Sestoft, P. (1997). "Deriving a Lazy Abstract Machine." *Journal of
   Functional Programming*, 7(3), pp. 231–264.

5. Jones, R.E. (1992). "Tail Recursion Without Space Leaks." *Journal of
   Functional Programming*, 2(1), pp. 73–79.

6. Friedman, D.P., Ghuloum, A., Siek, J.G. & Winebarger, O.L. (2007).
   "Improving the Lazy Krivine Machine." *Higher-Order and Symbolic
   Computation*, 20, pp. 271–293.

7. Peyton Jones, S.L. (1992). "Implementing Lazy Functional Languages on
   Stock Hardware: The Spineless Tagless G-machine." *Journal of Functional
   Programming*, 2(2), pp. 127–202.

8. Marlow, S. & Peyton Jones, S. (2006). "Making a Fast Curry: Push/Enter
   vs. Eval/Apply for Higher-Order Languages." *Journal of Functional
   Programming*, 16(4–5), pp. 415–449.

9. Peyton Jones, S.L. & Lester, D.R. (1992). *Implementing Functional
   Languages: A Tutorial*. Prentice Hall.
