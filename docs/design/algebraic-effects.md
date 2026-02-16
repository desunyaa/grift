# Algebraic Effects and Handlers for Grift

## A Design Document for One-Shot Delimited Continuations in a Pure Lazy Lisp

---

**Abstract.**
This document presents a design for extending Grift — a `no_std`, `no_alloc`,
arena-allocated pure lazy Lisp — with algebraic effects and handlers based on
one-shot delimited continuations. We address the unique challenges arising from
the interaction between call-by-need evaluation with memoising thunks and
effect handling, propose concrete syntax and operational semantics, and give a
detailed implementation strategy that respects Grift's constraints: a fixed-size
arena, `Copy`-only values, no heap allocation, and no unsafe code. The design
draws on the algebraic effects literature [1, 2, 3] while adapting the approach
to the realities of an embedded-friendly, arena-based runtime.

---

## Table of Contents

1. [Motivation](#1-motivation)
2. [Background and Related Work](#2-background-and-related-work)
3. [Design Goals and Constraints](#3-design-goals-and-constraints)
4. [Syntax](#4-syntax)
5. [Informal Semantics](#5-informal-semantics)
6. [Formal Operational Semantics](#6-formal-operational-semantics)
7. [Interaction with Lazy Evaluation](#7-interaction-with-lazy-evaluation)
8. [Implementation Strategy](#8-implementation-strategy)
9. [Arena Representation](#9-arena-representation)
10. [Evaluator Changes](#10-evaluator-changes)
11. [Garbage Collection](#11-garbage-collection)
12. [Worked Examples](#12-worked-examples)
13. [Limitations and Future Work](#13-limitations-and-future-work)
14. [References](#14-references)

---

## 1. Motivation

Grift is a purely functional Lisp: there is no `set!`, no mutable state, and
no I/O primitives. While purity simplifies reasoning and enables aggressive
memoisation, it also means that any program requiring interaction with the
outside world — reading input, signaling errors, non-deterministic choice,
cooperative concurrency — must encode effects manually, typically via
continuation-passing style (CPS) or monadic plumbing.

Algebraic effects and handlers [1, 2] provide a principled, composable
mechanism for expressing and interpreting computational effects. They separate
the *declaration* of effects (what operations are available) from their
*interpretation* (what those operations mean), enabling:

- **Modular effect composition** — multiple effects can be combined without
  monad transformer stacks or manual CPS threading.
- **User-defined control flow** — exceptions, state, generators, coroutines,
  backtracking, and async I/O can all be expressed as effect handlers.
- **Referential transparency** — an effect is only "performed" relative to an
  enclosing handler; outside the handler, the computation remains pure.

For Grift specifically, algebraic effects provide a path to practical programs
(I/O, error handling, cooperative multitasking) while preserving the pure,
lazy evaluation model that makes the language interesting for embedded and
resource-constrained environments.

## 2. Background and Related Work

### 2.1 Algebraic Effects and Handlers

Plotkin and Power [1] introduced algebraic effects as a framework for
structuring computational effects using the theory of universal algebra. Plotkin
and Pretnar [2] later developed the notion of *handlers* for algebraic effects,
which provide a way to give meaning to effect operations via fold-like
interpretations over computations.

An algebraic effect system consists of:
- **Effect declarations**: a set of *operation signatures* (e.g., `Get : Unit → S`,
  `Put : S → Unit` for mutable state).
- **Perform**: an expression `perform op v` that invokes an operation, suspending
  the current computation and passing control to the nearest enclosing handler.
- **Handlers**: a construct `handle e with { op x k → e' ; ... ; return x → e_r }`
  that interprets effect operations by providing clauses for each operation and
  a return clause for pure values.

When `perform op v` is evaluated, the runtime captures the *delimited
continuation* up to the nearest enclosing handler for `op`, and passes both the
argument `v` and the captured continuation `k` to the handler clause. The
handler may invoke `k` (resume the computation), discard it (abort), or store
it for later use.

### 2.2 Delimited Continuations

Delimited continuations [4, 5] are a well-studied control mechanism. The key
primitives are:
- `reset` (or `prompt`): establishes a delimiter on the continuation.
- `shift` (or `control`): captures the continuation up to the nearest delimiter.

Algebraic effect handlers generalise `shift`/`reset` by associating captured
continuations with named operations rather than anonymous prompts [6].

**One-shot vs. multi-shot.** A *one-shot* (or *linear*) continuation may be
invoked at most once. This restriction:
- Avoids the need to copy the captured continuation (critical for Grift's
  `Copy`-only arena where deep-copying continuation frames would be expensive).
- Aligns with affine or linear type disciplines [7].
- Suffices for exceptions, state, generators, coroutines, and most practical
  effect patterns. Multi-shot continuations are needed primarily for
  backtracking and non-deterministic search.

We adopt one-shot continuations for Grift. This is the same choice made by
OCaml 5's effect handlers [8], Eff [3], and Koka [9] (which uses "named, scoped
handlers" with an optimization for tail-resumptive handlers).

### 2.3 Effects and Lazy Evaluation

The interaction between effects and lazy evaluation is subtle and has been
studied in the context of Haskell's `IO` monad [10] and more recently in
work on lazy algebraic effects [11, 12].

The central tension is: **when should an effect be performed?**

In a strict language, the answer is straightforward: effects occur in the
order they are written. In a lazy language, evaluation order is
demand-driven, so an effect inside a thunk is performed only when (and if)
the thunk is forced. This can lead to:

1. **Unpredictable effect ordering** — the order in which thunks are forced
   determines the order of effects.
2. **Effect duplication or loss** — if a thunk containing an effect is never
   forced, the effect is lost; if it's forced multiple times (absent
   memoisation), the effect is duplicated.
3. **Memoisation semantics** — once a thunk is forced and memoised, subsequent
   accesses see the memoised value, not re-performed effects.

Grift's memoising thunks (with the black-hole protocol for cycle detection)
provide a natural resolution: a thunk is forced at most once, and the result
is memoised. This means:
- Each effect within a thunk is performed exactly once (when first forced).
- The order of effects is determined by the order of forcing.
- Memoisation ensures referential transparency for the *result* of the
  effectful computation, even though the effect itself is a side-channel.

This behavior is analogous to Haskell's `unsafePerformIO` for memoised thunks,
but with the crucial difference that in Grift, all effects are mediated by
handlers, so the semantics remain well-defined.

### 2.4 Relevant Systems

| System    | Effects Model           | Continuations | Evaluation | Arena/GC |
|-----------|------------------------|---------------|------------|----------|
| Eff [3]   | Algebraic effects      | Multi-shot    | Strict     | OCaml GC |
| Koka [9]  | Named/scoped handlers  | One-shot†     | Strict     | Ref-count|
| Frank [13]| Implicit handlers      | Multi-shot    | Strict     | GHC RTS  |
| OCaml 5 [8]| Effect handlers       | One-shot      | Strict     | OCaml GC |
| Links [14]| Row-based effects      | Multi-shot    | Strict     | Custom GC|
| **Grift** | **Algebraic effects**  | **One-shot**  | **Lazy**   | **Arena**|

†Koka supports multi-shot through explicit copying.

## 3. Design Goals and Constraints

### 3.1 Design Goals

1. **Composable effects** — multiple effect handlers can be nested and
   composed without interference.
2. **One-shot continuations** — captured continuations may be invoked at most
   once, enforced at runtime.
3. **Compatibility with lazy evaluation** — effects interact predictably
   with call-by-need evaluation and memoising thunks.
4. **Minimal surface area** — a small number of new special forms, not a
   large standard library.
5. **No type system changes** — Grift is dynamically typed; effect safety is
   enforced at runtime (unhandled effects produce errors).

### 3.2 Grift-Specific Constraints

These constraints arise from Grift's `no_std`, `no_alloc`, arena-based
architecture:

| Constraint | Implication for Effects |
|---|---|
| **Fixed-size arena** (`Arena<Value, N>`) | Continuation frames must be arena-allocated; deep continuations consume arena space. |
| **`Copy` types only** | All new `Value` variants must implement `Copy`. No `Box`, `Vec`, or reference-counted pointers. |
| **No heap allocation** | Continuation chains, handler stacks, and effect declarations must live in the arena. |
| **No unsafe code** | `#![forbid(unsafe_code)]` — all control flow must use safe Rust. |
| **Memoising thunks** | Effects inside thunks are performed once on first force, then memoised. |
| **Tail-call optimisation** | The trampoline loop (`TailAction::Continue`) must be preserved; handlers should not break TCO for tail-resumptive operations. |
| **GC via mark-and-sweep** | Continuation frames must be traceable by the garbage collector. |
| **Two-field `Value` variants** | Each `Value` variant can inline at most two `ArenaIndex`-sized fields (to keep `Value` `Copy` and small). |

## 4. Syntax

We introduce four new special forms. The syntax is designed to be minimal and
Lisp-natural.

### 4.1 Effect Declaration

```scheme
(defeffect <effect-name> (<operation-name> ...) ...)
```

Declares a named effect with one or more operations. Each operation takes one
argument and returns one value (following the convention of Plotkin and
Pretnar [2]).

**Example:**
```scheme
(defeffect state
  (get)        ; get : Unit → Value
  (put v))     ; put : Value → Unit

(defeffect exception
  (raise e))   ; raise : Value → ⊥

(defeffect choice
  (decide))    ; decide : Unit → Bool
```

**Semantics:** `defeffect` registers the effect name and its operations in
the global environment. Each operation becomes a callable that, when applied,
performs the corresponding effect.

### 4.2 Perform

```scheme
(perform <operation-name> <arg>)
(perform <operation-name>)        ; shorthand: arg defaults to ()
```

Invokes an effect operation. Suspends the current computation and transfers
control to the nearest enclosing handler for the operation's effect.

**Example:**
```scheme
(perform get)           ; invoke the Get operation
(perform put 42)        ; invoke Put with argument 42
(perform raise "error") ; invoke Raise with a string argument
```

### 4.3 Handle

```scheme
(handle <expr>
  (<operation-name> <arg-var> <continuation-var> <handler-body>) ...
  (return <result-var> <return-body>))
```

Evaluates `<expr>` under a handler that intercepts effect operations.

- Each operation clause receives the argument and a one-shot continuation.
- The `return` clause wraps the final result of `<expr>` if it completes
  without performing an (unhandled) effect.
- The continuation `k` is a one-shot function: `(k value)` resumes the
  computation with `value` as the result of the `perform` expression.

**Example: Exception handling**
```scheme
(handle (begin
          (perform raise "oops")
          42)  ; never reached
  (raise e k 
    (cons (quote error) e))  ; return error pair, discard k
  (return x x))              ; pass through pure results
```

**Example: Mutable state**
```scheme
(define (stateful-computation)
  (perform put 10)
  (+ (perform get) 1))

(define (run-state init body)
  (handle (body)
    (get _ k 
      (lambda (s) ((k s) s)))        ; resume with current state
    (put new-s k 
      (lambda (_) ((k ()) new-s)))   ; resume with (), update state
    (return x 
      (lambda (s) x))))              ; ignore final state, return value

;; Usage:
;; ((run-state 0 stateful-computation) 0)  =>  11
```

### 4.4 Resume

The continuation variable `k` bound in a handler clause is an ordinary
one-shot function. Invoking it is simply function application:

```scheme
(k value)   ; resume the suspended computation with 'value'
```

There is no separate `resume` keyword. This follows the design of Eff [3]
and OCaml 5 [8], where the continuation is a first-class (but linear) value.

Attempting to invoke `k` more than once results in a runtime error
(`ContinuationAlreadyResumed`).

## 5. Informal Semantics

### 5.1 Evaluation Rules (Informal)

1. **`(defeffect E (op₁ ...) ...)`** — registers effect `E` and its
   operations in the global environment. Each `opᵢ` is bound to a
   special "effect operation" value.

2. **`(perform op v)`** — looks up `op` in the environment to find its
   associated effect. Searches the handler stack for the nearest handler
   that handles this effect. Captures the continuation from the current
   evaluation point up to that handler (a *delimited continuation*).
   Invokes the handler clause for `op` with argument `v` and
   continuation `k`.

3. **`(handle e (op x k body) ... (return r rbody))`** — pushes a handler
   frame onto the handler stack, evaluates `e`. If `e` returns a value `v`
   without performing a handled effect, evaluates `rbody` with `r` bound to
   `v`. If `e` performs an operation `op` that is handled, evaluates the
   corresponding `body` with `x` bound to the argument and `k` bound to
   the captured continuation.

4. **`(k v)`** — resumes the captured continuation with value `v`. The
   continuation is consumed (one-shot). If `k` has already been invoked,
   raises `ContinuationAlreadyResumed`.

### 5.2 Handler Search

When `perform op v` is evaluated, the runtime searches the *handler stack*
(a chain of handler frames, innermost first) for a handler that provides a
clause for `op`. If no handler is found, the evaluator returns an
`UnhandledEffect` error.

This search proceeds through dynamically nested handlers, which may belong
to different effects. A handler for effect `E₁` that does not handle `op`
(belonging to effect `E₂`) is transparent: the search continues outward.

### 5.3 Continuation Scope

The captured continuation `k` includes:
- All evaluation frames between the `perform` site and the handler.
- The environment at each frame.
- Any intermediate handler frames (which are re-installed when `k` is
  resumed, so that effects within the continuation are properly handled).

When `k` is resumed, evaluation continues as if `perform op v` had returned
the value passed to `k`. Any handler frames that were part of the captured
continuation are re-installed on the handler stack.

## 6. Formal Operational Semantics

We present a small-step operational semantics for the core effect constructs.
The semantics is adapted from Pretnar's tutorial [15] and Hillerström and
Lindley's work on effect handlers with delimited control [6].

### 6.1 Syntax of the Core Calculus

```
v ::= n | #t | #f | () | (lambda (x ...) e) | (cont F)    Values
e ::= v | x | (e e) | (perform op e)                       Expressions
    | (handle e H)
    | (if e e e) | (begin e ...) | ...

H ::= { op₁ x₁ k₁ → e₁ ; ... ; return r → e_r }          Handler

F ::= □                                                     Frames
    | (F e) | (v F)                                         Application frames
    | (perform op F)                                        Perform arg frame
    | (handle F H)                                          Handler frame
    | ...
```

### 6.2 Evaluation Contexts

An *evaluation context* `E` is a term with a single hole `□` marking the
next redex. In the context of Grift's lazy evaluation, forcing a thunk
introduces an implicit evaluation context.

A *pure evaluation context* `P` is an evaluation context that contains no
`handle` frames — i.e., the portion of the continuation between the current
redex and the nearest enclosing handler.

### 6.3 Reduction Rules

**Handler installation:**
```
(handle v H)  ⟶  H.return[v]
```
When the body of a handler returns a value, apply the return clause.

**Perform (handler found):**
```
(handle P[(perform op v)] { ...; op x k → e; ...; return r → e_r })
  ⟶  e[x := v, k := (cont P)]
```
where `P` is the pure evaluation context between the `perform` and the
handler. The continuation `(cont P)` is a one-shot function.

**Continuation resumption:**
```
((cont P) v)  ⟶  P[v]
```
where the continuation is consumed (subsequent applications are errors).

**Perform (no handler):**
```
perform op v  ⟶  error(UnhandledEffect(op))
```
If no enclosing handler provides a clause for `op`.

**Effect forwarding:**
```
(handle P[(perform op v)] H)  ⟶  (perform op v)  if op ∉ dom(H)
```
with the continuation extended to include the handler frame:
```
  ⟶  (perform op v)  [continuation includes (handle P[□] H)]
```

In practice, the handler is transparent to unhandled operations: the search
continues to the next enclosing handler, and the transparent handler becomes
part of the captured continuation so it is re-installed on resumption.

### 6.4 Thunk Interaction

When a thunk `⟨e, ρ⟩` is forced inside a handler:

```
(handle P[force(⟨e, ρ⟩)] H)
  ⟶  (handle P[eval(e, ρ)] H)     [black-hole protocol]
```

If `e` performs an effect during forcing, the thunk's black-hole marker
remains in place until the handler resumes the continuation and the
thunk is resolved. This means:

1. The thunk is *not* memoised until the continuation completes (or is
   discarded).
2. If the handler discards the continuation, the thunk remains a black
   hole — subsequent attempts to force it will raise
   `BlackHoleDetected`.
3. If the handler resumes the continuation, the thunk is eventually
   memoised with the result, as in the non-effectful case.

This behavior is by design: it prevents partial memoisation of effectful
computations and preserves the black-hole cycle-detection invariant.

## 7. Interaction with Lazy Evaluation

### 7.1 The Lazy Effects Problem

In a call-by-need language, arguments to functions are wrapped in thunks
and forced only when needed. If a thunk contains an effectful computation,
the effect is performed at forcing time, not at the call site. This creates
a tension between:

- **Predictable effect ordering** (desirable for I/O, state).
- **Demand-driven evaluation** (fundamental to laziness).

### 7.2 Design Decision: Effects at Forcing Time

We adopt the following semantics:

> **An effect inside a thunk is performed when the thunk is first forced.**

This is the natural semantics for a lazy language and preserves the
memoisation invariant. The implications are:

1. **Effect order = forcing order.** The order in which effects occur is
   determined by the evaluation strategy, not the textual order of
   expressions. This is consistent with Haskell's approach to I/O via
   monads, where sequencing is explicit.

2. **At-most-once effects.** Because Grift's thunks are memoised after
   forcing (via the black-hole protocol and indirection), each effectful
   thunk performs its effect exactly once.

3. **Handler scope at forcing time.** When a thunk is forced, the active
   handler stack at forcing time determines which handler intercepts the
   effect. This can differ from the handler stack at thunk creation time.

### 7.3 Strict Effect Boundaries

For cases where predictable effect ordering is required, users should
use `begin` to sequence effects explicitly:

```scheme
(handle
  (begin
    (perform put 1)
    (perform put 2)
    (perform get))
  ...)
```

In `begin`, each sub-expression is evaluated in order (the result of
non-final expressions is discarded), so effects occur in textual order.

### 7.4 Effects Inside Lazy Cons

Because `cons` in Grift is lazy (both `car` and `cdr` are thunked), effects
inside a cons cell are deferred:

```scheme
(handle
  (let ((p (cons (perform get) (perform get))))
    ;; Neither 'get' has been performed yet.
    ;; Forcing (car p) performs the first 'get'.
    ;; Forcing (cdr p) performs the second 'get'.
    (+ (car p) (cdr p)))
  ...)
```

This is a feature, not a bug: it enables lazy effectful data structures
(e.g., effectful streams). However, users must be aware that the order of
effects in lazy data structures depends on access patterns.

## 8. Implementation Strategy

### 8.1 Overview

The implementation extends Grift's existing trampoline-based evaluator with:

1. **A handler stack** — a linked list of handler frames in the arena,
   threaded through the evaluator as additional state.
2. **Continuation reification** — when `perform` is evaluated, the
   evaluator captures the continuation as an arena-allocated chain of
   *continuation frames*.
3. **Continuation application** — when a captured continuation is applied
   (resumed), the evaluator reinstalls the continuation frames and
   continues evaluation.

### 8.2 Architectural Choices

**Why not CPS transform?** A global CPS transform would eliminate the need
for continuation capture at runtime, but it would:
- Break Grift's trampoline-based TCO (the CPS transform introduces
  administrative redexes that defeat the simple `TailAction` loop).
- Require transforming all user code and built-in functions.
- Increase arena pressure (every intermediate value needs a continuation
  closure).

Instead, we use a *direct-style* evaluator with *explicit continuation
reification* at `perform` sites, following the approach of OCaml 5 [8] and
libhandler [16].

**Why not stack copying?** Grift has no runtime call stack in the traditional
sense — the evaluator loop uses tail calls and explicit GC root management.
There is no contiguous stack to copy. Instead, we reify the continuation as
data in the arena.

### 8.3 Continuation Representation

A captured continuation is a *chain of continuation frames*, each
representing one step of the suspended evaluation. The key insight is that
Grift's evaluator already uses an explicit loop with `(expr, env)` state;
we extend this with a frame that records "what to do next" when a sub-result
is available.

Each continuation frame records:
- The **kind** of pending operation (apply function, force thunk,
  evaluate body of `begin`, etc.).
- The **saved data** needed to resume (e.g., the remaining expressions,
  the environment, the function to apply).
- A **link** to the next frame (the rest of the continuation).

## 9. Arena Representation

### 9.1 New Value Variants

We add the following variants to the `Value` enum:

```rust
pub enum Value {
    // ... existing variants ...

    /// An effect operation (created by defeffect).
    /// `effect` points to the effect declaration, `op_index` identifies the
    /// operation within that effect.
    EffectOp {
        effect: ArenaIndex,  // points to the effect name (symbol)
        op_id: ArenaIndex,   // points to the operation name (symbol)
    },

    /// A handler frame on the handler stack.
    /// `clauses` is a list of (op-name arg-var k-var body) tuples.
    /// `rest` is the next handler on the stack (or NIL).
    Handler {
        clauses: ArenaIndex, // list of operation clauses + return clause
        rest: ArenaIndex,    // next handler frame (linked list)
    },

    /// A one-shot delimited continuation.
    /// `frames` is the chain of continuation frames.
    /// `handler_segment` is the portion of the handler stack captured.
    Continuation {
        frames: ArenaIndex,    // linked list of continuation frames
        handler_seg: ArenaIndex, // captured handler frames
    },

    /// A continuation frame (one step of a suspended evaluation).
    /// `kind_and_data` encodes the frame kind and saved data.
    /// `next` links to the next frame.
    ContFrame {
        data: ArenaIndex, // cons cell encoding frame kind and saved state
        next: ArenaIndex, // next frame in the chain
    },
}
```

All new variants contain exactly two `ArenaIndex` fields, satisfying Grift's
constraint that `Value` variants inline at most two fields. Complex data
(e.g., handler clauses, frame state) is stored as cons-cell lists in the
arena and referenced by index.

### 9.2 Value Size Invariant

The existing `Value` enum has variants with at most two `ArenaIndex` fields
(e.g., `Cons { car, cdr }`, `Lambda { params, body_env }`,
`Thunk { expr, env }`). All new variants maintain this invariant:

| Variant | Field 1 | Field 2 |
|---|---|---|
| `EffectOp` | `effect: ArenaIndex` | `op_id: ArenaIndex` |
| `Handler` | `clauses: ArenaIndex` | `rest: ArenaIndex` |
| `Continuation` | `frames: ArenaIndex` | `handler_seg: ArenaIndex` |
| `ContFrame` | `data: ArenaIndex` | `next: ArenaIndex` |

### 9.3 Handler Clause Encoding

A handler's clauses are stored as a linked list of cons cells:

```
clauses = ((op-name . (arg-var . (k-var . (body . env)))) . rest-clauses)
```

The final element is the return clause:

```
return-clause = ((return-symbol . (result-var . (return-body . env))) . NIL)
```

This encoding uses only existing `Value::Cons` cells, keeping the
representation simple and GC-compatible.

### 9.4 Continuation Frame Encoding

Each `ContFrame`'s `data` field points to a cons cell encoding the frame
kind and saved state. The frame kinds correspond to the points in the
evaluator where sub-evaluations occur:

| Frame Kind | Encoded Data | Description |
|---|---|---|
| `apply-func` | `(func-whnf . (remaining-args . env))` | Applying a function: func evaluated, args pending |
| `apply-arg` | `(func-expr . (remaining-args . env))` | Evaluating function position |
| `begin-seq` | `(remaining-exprs . env)` | Sequencing: current expr done, more to go |
| `if-branch` | `(then-expr . (else-expr . env))` | Conditional: test evaluated, branch pending |
| `force-thunk` | `(thunk-idx . env)` | Forcing a thunk: recording which thunk to memoize |
| `handle-body` | `(handler-clauses . env)` | Inside a handler body |

The frame kind is encoded as a symbol in the `car` of the data cons cell,
enabling pattern matching during continuation resumption.

## 10. Evaluator Changes

### 10.1 Extended Evaluator State

```rust
pub(crate) struct Evaluator<'a, const N: usize> {
    lisp: &'a Lisp<N>,
    pub global_env: ArenaIndex,
    gc_roots: ArenaIndex,
    /// Handler stack: linked list of Handler values (innermost first).
    handler_stack: ArenaIndex,  // NEW
}
```

### 10.2 New Special Forms

The `define_builtins!` macro is extended:

```rust
define_builtins! {
    builtins {
        // ... existing builtins ...
    }
    special_forms {
        // ... existing special forms ...
        "defeffect" => eval_defeffect,
        "perform"   => eval_perform,
        "handle"    => eval_handle,
    }
}
```

### 10.3 defeffect Implementation

```rust
/// (defeffect <name> (<op> ...) ...)
fn eval_defeffect(
    &mut self,
    args: ArenaIndex,
    _expr: &mut ArenaIndex,
    _env: &mut ArenaIndex,
) -> TailAction {
    non_tail(self.eval_defeffect_inner(args))
}

fn eval_defeffect_inner(
    &mut self,
    args: ArenaIndex,
) -> ArenaResult<ArenaIndex> {
    let effect_name = self.lisp.car(args)?;  // symbol
    let ops_list = self.lisp.cdr(args)?;     // list of (op-name ...)

    // For each operation, create an EffectOp value and bind it globally
    let mut cur = ops_list;
    while !cur.is_nil() {
        let op_form = self.lisp.car(cur)?;
        let op_name = self.lisp.car(op_form)?;  // symbol

        let effect_op = self.lisp.arena.alloc(Value::EffectOp {
            effect: effect_name,
            op_id: op_name,
        })?;

        self.global_env = env_bind(
            self.lisp, self.global_env, op_name, effect_op
        )?;
        cur = self.lisp.cdr(cur)?;
    }

    Ok(effect_name)
}
```

### 10.4 handle Implementation

The `handle` form pushes a handler frame onto the handler stack, evaluates
the body, and pops the frame on completion.

```rust
/// (handle <expr> (<op> <arg> <k> <body>) ... (return <r> <rbody>))
fn eval_handle(
    &mut self,
    args: ArenaIndex,
    expr: &mut ArenaIndex,
    env: &mut ArenaIndex,
) -> TailAction {
    tail_continue!((|| -> ArenaResult<()> {
        let body_expr = self.lisp.car(args)?;
        let clause_list = self.lisp.cdr(args)?;

        // Attach current environment to each clause for closure capture
        let clauses = self.attach_env_to_clauses(clause_list, *env)?;

        // Push handler frame
        let handler = self.lisp.arena.alloc(Value::Handler {
            clauses,
            rest: self.handler_stack,
        })?;
        self.handler_stack = handler;

        // Evaluate body
        let result = self.eval(body_expr, *env)?;
        let forced = self.force(result)?;

        // Pop handler frame
        self.handler_stack = match self.lisp.get(self.handler_stack)? {
            Value::Handler { rest, .. } => rest,
            _ => return Err(ArenaError::TypeError),
        };

        // Apply return clause
        let return_clause = self.find_return_clause(clauses)?;
        let (result_var, return_body, return_env) = return_clause;
        let new_env = env_bind(self.lisp, return_env, result_var, forced)?;
        *expr = return_body;
        *env = new_env;
        Ok(())
    })())
}
```

### 10.5 perform Implementation

This is the most complex new operation. When `perform` is evaluated:

1. Look up the operation in the environment to get the `EffectOp` value.
2. Evaluate the argument (strictly).
3. Search the handler stack for a handler with a clause for this operation.
4. Capture the continuation from the current evaluation point up to the
   handler as a chain of `ContFrame` values.
5. Create a one-shot `Continuation` value.
6. Pop handlers up to (and including) the matching handler.
7. Invoke the handler clause with the argument and continuation.

```rust
/// (perform <op> <arg>)
fn eval_perform(
    &mut self,
    args: ArenaIndex,
    expr: &mut ArenaIndex,
    env: &mut ArenaIndex,
) -> TailAction {
    // The actual continuation capture requires cooperation from
    // the eval loop. See Section 10.6 for the full mechanism.
    //
    // Conceptually:
    // 1. Signal "effect performed" to the eval loop
    // 2. The eval loop unwinds, collecting continuation frames
    // 3. The handler clause is invoked with arg + continuation
    non_tail(self.eval_perform_inner(args, *env))
}
```

### 10.6 Continuation Capture Mechanism

The continuation capture mechanism must integrate with Grift's trampoline
loop. We extend `TailAction` with a new variant:

```rust
enum TailAction {
    Return(ArenaResult<ArenaIndex>),
    Continue,
    /// An effect was performed; unwind to the handler.
    Perform {
        op_name: ArenaIndex,   // operation name (symbol)
        arg: ArenaIndex,       // argument to the operation
    },
}
```

When `eval_perform` is called, it returns `TailAction::Perform { op_name, arg }`.
The eval loop propagates this upward through the call stack, collecting
continuation frames at each level. When the loop reaches the matching
handler, it:

1. Constructs a `Continuation` value from the collected frames.
2. Looks up the handler clause for the operation.
3. Binds the argument and continuation in the handler clause's environment.
4. Sets `expr` and `env` to evaluate the handler body.

### 10.7 Modified Eval Loop

The main eval loop is extended to handle `TailAction::Perform`:

```rust
pub fn eval(
    &mut self, mut expr: ArenaIndex, mut env: ArenaIndex
) -> ArenaResult<ArenaIndex> {
    loop {
        self.maybe_collect(expr, env);
        let val = self.lisp.get(expr)?;

        if val.is_self_evaluating() {
            return Ok(expr);
        }

        match val {
            // ... existing cases ...

            Value::Cons { car, cdr } => {
                // ... existing special form dispatch ...

                // Function application
                let func_whnf = self.eval_force(car, env)?;

                match self.lisp.get(func_whnf)? {
                    // Continuation application (one-shot)
                    Value::Continuation { .. } => {
                        let arg_expr = self.lisp.car(cdr)?;
                        let arg = self.eval_force(arg_expr, env)?;
                        return self.resume_continuation(func_whnf, arg);
                    }

                    // ... existing Builtin and Lambda cases ...
                    _ => { /* ... */ }
                }
            }

            _ => unreachable!(),
        }
    }
}
```

### 10.8 One-Shot Enforcement

When a continuation is resumed, the `Continuation` value in the arena is
overwritten with a sentinel:

```rust
fn resume_continuation(
    &mut self,
    cont_idx: ArenaIndex,
    arg: ArenaIndex,
) -> ArenaResult<ArenaIndex> {
    let Value::Continuation { frames, handler_seg } =
        self.lisp.get(cont_idx)?
    else {
        return Err(ArenaError::TypeError);
    };

    // One-shot enforcement: mark as consumed
    self.lisp.arena.set(
        cont_idx,
        Value::BlackHole,  // Reuse BlackHole as "consumed" marker
    )?;

    // Reinstall captured handler segment
    self.reinstall_handlers(handler_seg)?;

    // Replay continuation frames with `arg` as the initial value
    self.replay_frames(frames, arg)
}
```

Attempting to resume a consumed continuation (now a `BlackHole`) will trigger
the existing `BlackHoleDetected` error, which is a natural fit:

```
Error: Attempted to resume a one-shot continuation more than once
```

This could alternatively use a dedicated error variant
(`ContinuationAlreadyResumed`) for clearer error messages.

### 10.9 Error Variant Extension

The `ArenaError` enum is extended:

```rust
pub enum ArenaError {
    // ... existing variants ...

    /// An effect operation was performed but no handler was found.
    UnhandledEffect,

    /// A one-shot continuation was invoked more than once.
    ContinuationAlreadyResumed,
}
```

## 11. Garbage Collection

### 11.1 Tracing New Value Variants

The `Trace` implementation for `Value` must be extended to trace through
the new variants:

```rust
impl<const N: usize> Trace<Value, N> for Value {
    fn trace<F: FnMut(ArenaIndex)>(&self, mut tracer: F) {
        match *self {
            // ... existing cases ...

            Value::EffectOp { effect, op_id } => {
                tracer(effect);
                tracer(op_id);
            }
            Value::Handler { clauses, rest } => {
                tracer(clauses);
                tracer(rest);
            }
            Value::Continuation { frames, handler_seg } => {
                tracer(frames);
                tracer(handler_seg);
            }
            Value::ContFrame { data, next } => {
                tracer(data);
                tracer(next);
            }
        }
    }
}
```

Because all new variants follow the two-`ArenaIndex` pattern, they integrate
naturally with the existing `Trace` implementation (which already handles
`Cons`, `Lambda`, and `Thunk` in a single match arm for two-field variants).

### 11.2 GC Roots

The handler stack must be added as a GC root:

```rust
fn collect_garbage(&self, expr: ArenaIndex, env: ArenaIndex) {
    self.lisp.arena.collect_garbage(&[
        expr,
        env,
        self.global_env,
        self.gc_roots,
        self.handler_stack,  // NEW
    ]);
}
```

Captured continuations are reachable through the values they are bound to
(in handler clause environments), so they do not need special root treatment
— they are traced through the normal environment chain.

### 11.3 Arena Pressure

Continuations consume arena space proportional to the depth of the
suspended computation. For deeply nested computations, this can lead to
increased memory pressure. The existing GC trigger (75% capacity) will
handle this, but users should be aware that heavy use of effects in
deeply recursive computations may require a larger arena.

**Mitigation:** Tail-resumptive handlers (where the handler clause
tail-calls the continuation `k`) can be optimized to avoid capturing
the continuation at all, similar to Koka's tail-resumption optimization
[9]. See Section 13 for future work.

## 12. Worked Examples

### 12.1 Exception Handling

```scheme
;; Define an exception effect
(defeffect exn (raise))

;; A computation that may fail
(define (safe-div a b)
  (if (= b 0)
      (perform raise (quote division-by-zero))
      (/ a b)))

;; Handle exceptions
(handle (safe-div 10 0)
  (raise e k (cons (quote error) e))
  (return x (cons (quote ok) x)))
;; => (error . division-by-zero)

(handle (safe-div 10 2)
  (raise e k (cons (quote error) e))
  (return x (cons (quote ok) x)))
;; => (ok . 5)
```

**Execution trace for `(safe-div 10 0)`:**
1. `handle` pushes handler frame H₁.
2. `safe-div` is called with `a=10, b=0`.
3. `(= b 0)` evaluates to `#t`.
4. `(perform raise (quote division-by-zero))` is evaluated.
5. Handler search finds H₁ with clause for `raise`.
6. Continuation `k` captures the context `□` (nothing between perform and handler).
7. Handler clause: `e` = `division-by-zero`, `k` = captured continuation.
8. Handler body `(cons (quote error) e)` evaluates to `(error . division-by-zero)`.
9. `k` is never invoked (discarded).

### 12.2 Stateful Computation

```scheme
(defeffect state (get) (put))

;; Run a stateful computation with initial state
(define (run-state init comp)
  (handle (comp)
    (get _ k
      (lambda (s) ((k s) s)))
    (put new-s k
      (lambda (_) ((k ()) new-s)))
    (return x
      (lambda (s) x))))

;; A computation that uses state
(define (counter)
  (let ((n (perform get)))
    (perform put (+ n 1))
    (perform get)))

;; Execute
((run-state 0 counter) 0)  ;; => 1
```

**Execution trace:**
1. `run-state` evaluates `(handle (comp) ...)`.
2. `comp` evaluates to `counter`, which is forced.
3. `(perform get)` captures continuation `k₁ = □ → let n = □ in ...`.
4. Handler: `(lambda (s) ((k₁ s) s))` — a state-threading function.
5. When applied to state `0`: `(k₁ 0)` resumes with `n = 0`.
6. `(perform put (+ 0 1))` = `(perform put 1)` captures `k₂ = □ → (perform get)`.
7. Handler: `(lambda (_) ((k₂ ()) 1))` — continues with state `1`.
8. `(k₂ ())` resumes, evaluates `(perform get)` with state `1`.
9. Captures `k₃ = □` (trivial continuation).
10. Handler: `(lambda (s) ((k₃ s) s))` → `(lambda (s) (s s))` → applied with `1`.
11. `k₃` resumes with `1`, return clause: `(lambda (s) 1)`.
12. Applied to state `1`: result is `1`.

### 12.3 Generator / Iterator

```scheme
(defeffect yield (yield))

;; A generator that yields values 1, 2, 3
(define (gen123)
  (perform yield 1)
  (perform yield 2)
  (perform yield 3))

;; Collect all yielded values into a list
(define (collect gen)
  (handle (begin (gen) (quote done))
    (yield v k
      (cons v (collect (lambda () (k ())))))
    (return x (list))))

(collect gen123)  ;; => (1 2 3)
```

### 12.4 Effects with Lazy Data Structures

```scheme
(defeffect log (log))

;; Build a lazy list where each element logs when forced
(define (make-logging-list n)
  (if (= n 0)
      ()
      (cons (begin (perform log n) n)
            (make-logging-list (- n 1)))))

;; Only forcing elements triggers the log effect
(handle
  (let ((xs (make-logging-list 5)))
    ;; Only car is forced, so only 5 is logged
    (car xs))
  (log v k
    (begin
      ;; In a real system, this would output to a log
      (k ())))
  (return x x))
;; => 5 (and log effect performed once with value 5)
```

## 13. Limitations and Future Work

### 13.1 Current Limitations

1. **One-shot only.** Multi-shot continuations (needed for backtracking,
   non-deterministic search, probabilistic programming) are not supported.
   Supporting multi-shot would require deep-copying continuation frames,
   which is expensive in the arena model.

2. **No static effect typing.** Grift is dynamically typed, so unhandled
   effects are caught at runtime, not compile time. A future type system
   could use row-typed effects [17] to provide static guarantees.

3. **Arena pressure from continuations.** Deep continuations consume
   significant arena space. Programs with deeply nested effect handling
   may need larger arenas.

4. **No tail-resumption optimization.** Handlers that immediately
   tail-call the continuation (`(k v)` in tail position) still capture
   and restore the full continuation. Koka's tail-resumption optimization
   [9] could avoid this overhead.

### 13.2 Future Work

1. **Tail-resumptive handler optimization.** Detect handler clauses of the
   form `(op x k (k e))` and optimize them to avoid continuation capture,
   instead evaluating `e` and returning directly. This is equivalent to
   Koka's "tail-resumptive" optimization and would significantly reduce
   overhead for common patterns like state handlers.

2. **Shallow handlers.** Shallow handlers [18] (which handle only the first
   occurrence of an effect, requiring explicit re-installation) can be more
   efficient than deep handlers for certain patterns. They are a natural
   extension of the current design.

3. **Effect polymorphism.** Allow functions to be polymorphic over effects,
   enabling generic combinators like `map` to work with effectful functions
   without knowing which effects they use.

4. **Multi-shot continuations.** Support multi-shot via explicit
   `copy-continuation` operation, using the arena's `ArenaCopy` trait to
   deep-copy continuation frames. This would be expensive but correct.

5. **Scoped effects.** Following Koka's design [9], associate effects with
   lexically scoped labels rather than dynamic handler search. This would
   provide more predictable semantics and enable static effect resolution
   in some cases.

6. **Effect-aware GC.** Suspended continuations that are known to be
   unreachable (e.g., exception handlers where the body completed normally)
   could be eagerly freed rather than waiting for the next GC cycle.

## 14. References

[1] G. Plotkin and J. Power, "Adequacy for Algebraic Effects,"
    *Foundations of Software Science and Computation Structures (FoSSaCS)*,
    pp. 1–24, 2001.

[2] G. Plotkin and M. Pretnar, "Handlers of Algebraic Effects,"
    *European Symposium on Programming (ESOP)*, pp. 80–94, 2009.

[3] A. Bauer and M. Pretnar, "Programming with Algebraic Effects and
    Handlers," *Journal of Logical and Algebraic Methods in Programming*,
    vol. 84, no. 1, pp. 108–123, 2015.

[4] O. Danvy and A. Filinski, "Abstracting Control," *ACM Conference on
    LISP and Functional Programming (LFP)*, pp. 151–160, 1990.

[5] M. Felleisen, "The Theory and Practice of First-Class Prompts,"
    *Principles of Programming Languages (POPL)*, pp. 180–190, 1988.

[6] D. Hillerström and S. Lindley, "Liberating Effects with Rows and
    Handlers," *Workshop on Type-Driven Development (TyDe)*,
    pp. 15–27, 2016.

[7] J. Maurer, D. Downen, Z. M. Ariola, and S. Peyton Jones,
    "Compiling without Continuations," *ACM SIGPLAN Conference on
    Programming Language Design and Implementation (PLDI)*,
    pp. 482–494, 2017.

[8] K. Sivaramakrishnan, S. Dolan, L. White, S. Jaffer, T. Kelly,
    A. Sahoo, S. Parimala, A. Dhiman, and A. Madhavapeddy, "Retrofitting
    Effect Handlers onto OCaml," *ACM SIGPLAN Conference on Programming
    Language Design and Implementation (PLDI)*, pp. 206–221, 2021.

[9] D. Leijen, "Type Directed Compilation of Row-Typed Algebraic Effects,"
    *Principles of Programming Languages (POPL)*, pp. 486–499, 2017.

[10] S. Peyton Jones, "Tackling the Awkward Squad: Monadic Input/Output,
     Concurrency, Exceptions, and Foreign-Language Calls in Haskell,"
     *Engineering Theories of Software Construction*, pp. 47–96, 2001.

[11] S. Lindley, C. McBride, and C. McLaughlin, "Do Be Do Be Do,"
     *Principles of Programming Languages (POPL)*, pp. 500–514, 2017.

[12] J. Brachthäuser, P. Schuster, and K. Ostermann, "Effects as
     Capabilities: Effect Handlers and Lightweight Effect Polymorphism,"
     *Object-Oriented Programming, Systems, Languages & Applications
     (OOPSLA)*, pp. 126:1–126:30, 2020.

[13] C. McBride, "Frank, Earnestly," *Workshop on Higher-Order
     Programming with Effects*, 2012.

[14] D. Hillerström, S. Lindley, and R. Atkey, "Effect Handlers via
     Generalised Continuations," *Journal of Functional Programming*,
     vol. 30, e5, 2020.

[15] M. Pretnar, "An Introduction to Algebraic Effects and Handlers,"
     *Electronic Notes in Theoretical Computer Science*, vol. 319,
     pp. 19–35, 2015.

[16] D. Leijen, "Implementing Algebraic Effects in C," *Asian Symposium
     on Programming Languages and Systems (APLAS)*, pp. 339–363, 2017.

[17] S. Lindley, C. McBride, and C. McLaughlin, "Do Be Do Be Do,"
     *Principles of Programming Languages (POPL)*, pp. 500–514, 2017.

[18] D. Hillerström and S. Lindley, "Shallow Effect Handlers,"
     *Asian Symposium on Programming Languages and Systems (APLAS)*,
     pp. 415–435, 2018.
