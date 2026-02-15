//! Trampoline-based CPS evaluator.
//!
//! The evaluation loop maintains three mutable `ArenaIndex` values —
//! current expression, environment, and continuation — and never
//! recurses on the Rust stack for tail calls.

use grift_arena::{Arena, ArenaIndex};
use crate::value::{Value, Purity, PrimId};
use crate::pack::*;
use crate::env::{env_lookup, extend_env, env_define};
use crate::thunk::{thunk_arg_list, reverse_cons_list};
use crate::purity::analyze_purity;
use crate::primitives::apply_primitive;
use crate::intern::sym_eq;
use crate::error::{EvalError, ErrorKind};

// ============================================================================
// Bounce — trampoline state
// ============================================================================

/// Trampoline bounce.
enum Bounce {
    /// Continue evaluating.
    Continue { expr: ArenaIndex, env: ArenaIndex, cont: ArenaIndex },
    /// Evaluation is done; this is the final value.
    Done(ArenaIndex),
}

// ============================================================================
// Public entry point
// ============================================================================

/// Evaluate an expression in the given environment.
///
/// Returns the arena index of the result value.
/// The result is fully forced (no thunks).
pub fn run(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    expr: ArenaIndex,
    env: ArenaIndex,
) -> Result<ArenaIndex, EvalError> {
    let cont = arena.alloc(Value::ContHalt).unwrap();
    let mut cur_expr = expr;
    let mut cur_env = env;
    let mut cur_cont = cont;

    loop {
        match step(arena, cur_expr, cur_env, cur_cont)? {
            Bounce::Continue { expr: e, env: en, cont: c } => {
                cur_expr = e;
                cur_env = en;
                cur_cont = c;
            }
            Bounce::Done(result) => {
                // Force any remaining thunks in the result
                return force_result(arena, result);
            }
        }
    }
}

/// Force a result value, resolving thunks and Evaluated redirects.
fn force_result(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    idx: ArenaIndex,
) -> Result<ArenaIndex, EvalError> {
    let mut cur = idx;
    loop {
        match arena.get(cur).unwrap() {
            Value::Evaluated { value } => cur = value,
            Value::Thunk { expr, env } => {
                // Force the thunk by evaluating it
                let expr = expr; let env = env;
                let result = run(arena, expr, env)?;
                let _ = arena.set(cur, Value::Evaluated { value: result });
                cur = result;
            }
            _ => return Ok(cur),
        }
    }
}

// ============================================================================
// step
// ============================================================================

/// Evaluate one sub-expression and return a `Bounce`.
fn step(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    expr: ArenaIndex,
    env: ArenaIndex,
    cont: ArenaIndex,
) -> Result<Bounce, EvalError> {
    match arena.get(expr).unwrap() {
        // Literals → pass directly to continuation
        Value::Int(_) | Value::Bool(_) | Value::Nil => {
            apply_cont(arena, cont, expr)
        }

        // Symbol → look up in environment; if result is a thunk, force it
        Value::Symbol(s) => {
            let val = env_lookup(arena, env, s)?;
            match arena.get(val).unwrap() {
                Value::Thunk { expr: te, env: tenv } => {
                    let te = te; let tenv = tenv;
                    let fc = arena.alloc(Value::ContForce { slot: val, next: cont }).unwrap();
                    Ok(Bounce::Continue { expr: te, env: tenv, cont: fc })
                }
                Value::Evaluated { value } => {
                    apply_cont(arena, cont, value)
                }
                _ => apply_cont(arena, cont, val),
            }
        }

        // Thunk → force it
        Value::Thunk { expr: te, env: tenv } => {
            let te = te; let tenv = tenv;
            let fc = arena.alloc(Value::ContForce { slot: expr, next: cont }).unwrap();
            Ok(Bounce::Continue { expr: te, env: tenv, cont: fc })
        }

        // Already evaluated thunk redirect
        Value::Evaluated { value } => {
            apply_cont(arena, cont, value)
        }

        // Cons → dispatch as special form or function application
        Value::Cons { head, tail } => {
            let head = head; let tail = tail;
            dispatch_form(arena, head, tail, env, cont)
        }

        // Already a value (function, primitive, etc.)
        _ => apply_cont(arena, cont, expr),
    }
}

// ============================================================================
// apply_cont
// ============================================================================

/// Deliver a value to a continuation.
fn apply_cont(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    cont: ArenaIndex,
    value: ArenaIndex,
) -> Result<Bounce, EvalError> {
    match arena.get(cont).unwrap() {
        Value::ContHalt => Ok(Bounce::Done(value)),

        Value::ContForce { slot, next } => {
            let slot = slot; let next = next;
            // Overwrite the thunk slot with an Evaluated redirect
            let _ = arena.set(slot, Value::Evaluated { value });
            // If the result is itself a thunk, chain forcing
            match arena.get(value).unwrap() {
                Value::Thunk { expr, env } => {
                    let expr = expr; let env = env;
                    let fc = arena.alloc(Value::ContForce { slot, next }).unwrap();
                    Ok(Bounce::Continue { expr, env, cont: fc })
                }
                _ => apply_cont(arena, next, value),
            }
        }

        Value::ContIf { data, next } => {
            let d = unpack_cont_if(arena, data);
            let next = next;
            match arena.get(value).unwrap() {
                Value::Bool(true) => Ok(Bounce::Continue { expr: d.then_expr, env: d.env, cont: next }),
                Value::Bool(false) => Ok(Bounce::Continue { expr: d.else_expr, env: d.env, cont: next }),
                // Force non-bool condition (may be a thunk result)
                _ => Err(EvalError::new(ErrorKind::TypeError)),
            }
        }

        Value::ContEvalCallee { data, next } => {
            let d = unpack_cont_eval_callee(arena, data);
            let next = next;
            // `value` is the evaluated callee
            // Primitives always use strict evaluation except `cons`
            // which does not force its arguments.
            // Only user-defined pure functions use lazy evaluation.
            match arena.get(value).unwrap() {
                Value::Primitive { id: PrimId::ConsP, .. } => {
                    // cons does NOT force args — wrap in thunks
                    let thunked = thunk_arg_list(arena, d.arg_exprs, d.env);
                    apply_function(arena, value, thunked, next)
                }
                Value::Primitive { .. } => {
                    begin_strict_eval(arena, value, d.arg_exprs, d.env, next)
                }
                Value::Function { purity, .. } => {
                    match purity {
                        Purity::Pure => {
                            let thunked = thunk_arg_list(arena, d.arg_exprs, d.env);
                            apply_function(arena, value, thunked, next)
                        }
                        Purity::Impure => {
                            begin_strict_eval(arena, value, d.arg_exprs, d.env, next)
                        }
                    }
                }
                _ => {
                    // Unknown callee type
                    Err(EvalError::new(ErrorKind::TypeError))
                }
            }
        }

        Value::ContEvalArg { data, next } => {
            let d = unpack_cont_eval_arg(arena, data);
            let next = next;
            // Prepend just-evaluated value to evaluated_args
            let new_evald = arena.alloc(Value::Cons { head: value, tail: d.evaluated_args }).unwrap();

            match arena.get(d.remaining_args).unwrap() {
                Value::Nil => {
                    let final_args = reverse_cons_list(arena, new_evald);
                    apply_function(arena, d.func, final_args, next)
                }
                Value::Cons { head: next_arg, tail: rest } => {
                    let next_arg = next_arg; let rest = rest;
                    let new_data = pack_eval_arg_data(arena, d.func, new_evald, rest, d.env);
                    let new_cont = arena.alloc(Value::ContEvalArg { data: new_data, next }).unwrap();
                    Ok(Bounce::Continue { expr: next_arg, env: d.env, cont: new_cont })
                }
                _ => Err(EvalError::new(ErrorKind::Malformed)),
            }
        }

        Value::ContDo { data, next } => {
            let d = unpack_cont_do(arena, data);
            let next = next;
            match arena.get(d.remaining).unwrap() {
                Value::Nil => apply_cont(arena, next, value),
                Value::Cons { head: next_expr, tail: rest } => {
                    let next_expr = next_expr; let rest = rest;
                    let new_data = pack_cont_do_data(arena, rest, d.env);
                    let do_cont = arena.alloc(Value::ContDo { data: new_data, next }).unwrap();
                    Ok(Bounce::Continue { expr: next_expr, env: d.env, cont: do_cont })
                }
                _ => Err(EvalError::new(ErrorKind::Malformed)),
            }
        }

        Value::ContLet { data, next } => {
            let d = unpack_cont_let(arena, data);
            let next = next;
            // Bind the value
            let new_env = env_define(arena, d.env, d.name, value);

            match arena.get(d.remaining_bindings).unwrap() {
                Value::Nil => {
                    // All bindings done — evaluate body
                    Ok(Bounce::Continue { expr: d.body, env: new_env, cont: next })
                }
                Value::Cons { head: binding, tail: rest_bindings } => {
                    let binding = binding; let rest_bindings = rest_bindings;
                    // binding = Cons(name, Cons(init_expr, Nil))
                    let bname = cons_head(arena, binding);
                    let init_expr = cons_head(arena, cons_tail(arena, binding));
                    let new_data = pack_cont_let_data(arena, bname, rest_bindings, d.body, new_env);
                    let let_cont = arena.alloc(Value::ContLet { data: new_data, next }).unwrap();
                    Ok(Bounce::Continue { expr: init_expr, env: new_env, cont: let_cont })
                }
                _ => Err(EvalError::new(ErrorKind::Malformed)),
            }
        }

        Value::ContMatch { data, next } => {
            let d = unpack_cont_match(arena, data);
            let next = next;
            // Try each clause against `value`
            try_match_clauses(arena, value, d.clauses, d.env, next)
        }

        Value::ContDef { data, next } => {
            let d = unpack_cont_def(arena, data);
            let next = next;
            // Bind name in environment — but since environments are
            // immutable in this CPS model, we create a new env and
            // continue with it.  The new env becomes the "current" env
            // for subsequent expressions.
            let new_env = env_define(arena, d.env, d.name, value);
            // Return the value through the continuation, but with the
            // updated environment.  For top-level use, the caller
            // should use `eval_sequence` which threads the env.
            apply_cont(arena, next, new_env)
        }

        _ => Err(EvalError::new(ErrorKind::Internal)),
    }
}

// ============================================================================
// dispatch_form
// ============================================================================

/// Dispatch a list expression — either a special form or a function call.
fn dispatch_form(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    head: ArenaIndex,
    tail: ArenaIndex,
    env: ArenaIndex,
    cont: ArenaIndex,
) -> Result<Bounce, EvalError> {
    // Check for special forms by looking at the head symbol
    if let Value::Symbol(s) = arena.get(head).unwrap() {
        if sym_eq(arena, s, "quote") {
            // (quote <datum>) → return datum unevaluated
            let datum = cons_head(arena, tail);
            return apply_cont(arena, cont, datum);
        }
        if sym_eq(arena, s, "if") {
            // (if <test> <then> <else>)
            let test_expr = cons_head(arena, tail);
            let rest = cons_tail(arena, tail);
            let then_expr = cons_head(arena, rest);
            let rest2 = cons_tail(arena, rest);
            let else_expr = cons_head(arena, rest2);

            // Force the condition
            let data = pack_cont_if_data(arena, then_expr, else_expr, env);
            let if_cont = arena.alloc(Value::ContIf { data, next: cont }).unwrap();
            // Need to force condition — wrap in a force cont
            return Ok(Bounce::Continue { expr: test_expr, env, cont: if_cont });
        }
        if sym_eq(arena, s, "fn") {
            // (fn (<params...>) <body>)
            let params = cons_head(arena, tail);
            let body = cons_head(arena, cons_tail(arena, tail));
            let purity = analyze_purity(arena, body, env);
            let data = pack_function_data(arena, params, body, env);
            let func = arena.alloc(Value::Function { data, purity }).unwrap();
            return apply_cont(arena, cont, func);
        }
        if sym_eq(arena, s, "def") {
            // (def <name> <expr>) or (def (<name> <params...>) <body>)
            let first = cons_head(arena, tail);
            match arena.get(first).unwrap() {
                Value::Symbol(_) => {
                    // (def <name> <expr>)
                    let init_expr = cons_head(arena, cons_tail(arena, tail));
                    let data = pack_cont_def_data(arena, first, env);
                    let def_cont = arena.alloc(Value::ContDef { data, next: cont }).unwrap();
                    return Ok(Bounce::Continue { expr: init_expr, env, cont: def_cont });
                }
                Value::Cons { head: name, tail: params } => {
                    // (def (<name> <params...>) <body>)
                    // Create a self-referential closure:
                    // 1. Create a placeholder thunk for the function
                    // 2. Create new env with name -> placeholder
                    // 3. Create the function with the new env
                    // 4. Overwrite the placeholder with the actual function
                    let name = name; let params = params;
                    let body = cons_head(arena, cons_tail(arena, tail));
                    let purity = analyze_purity(arena, body, env);

                    // Placeholder that will be overwritten
                    let placeholder = arena.alloc(Value::Nil).unwrap();
                    let rec_env = env_define(arena, env, name, placeholder);

                    let fn_data = pack_function_data(arena, params, body, rec_env);
                    let func = arena.alloc(Value::Function { data: fn_data, purity }).unwrap();

                    // Overwrite placeholder with the actual function
                    let _ = arena.set(placeholder, Value::Evaluated { value: func });

                    let data = pack_cont_def_data(arena, name, env);
                    let def_cont = arena.alloc(Value::ContDef { data, next: cont }).unwrap();
                    return apply_cont(arena, def_cont, func);
                }
                _ => return Err(EvalError::new(ErrorKind::Malformed)),
            }
        }
        if sym_eq(arena, s, "do") {
            // (do <expr1> ... <exprN>)
            // Evaluate expressions in sequence, return the last value
            match arena.get(tail).unwrap() {
                Value::Nil => {
                    let nil = arena.alloc(Value::Nil).unwrap();
                    return apply_cont(arena, cont, nil);
                }
                Value::Cons { head: first, tail: rest } => {
                    let first = first; let rest = rest;
                    match arena.get(rest).unwrap() {
                        Value::Nil => {
                            // Only one expression — tail position
                            return Ok(Bounce::Continue { expr: first, env, cont });
                        }
                        _ => {
                            let data = pack_cont_do_data(arena, rest, env);
                            let do_cont = arena.alloc(Value::ContDo { data, next: cont }).unwrap();
                            return Ok(Bounce::Continue { expr: first, env, cont: do_cont });
                        }
                    }
                }
                _ => return Err(EvalError::new(ErrorKind::Malformed)),
            }
        }
        if sym_eq(arena, s, "let") {
            // (let ((<name> <expr>) ...) <body>)
            let bindings_list = cons_head(arena, tail);
            let body = cons_head(arena, cons_tail(arena, tail));

            match arena.get(bindings_list).unwrap() {
                Value::Nil => {
                    // No bindings — evaluate body directly
                    return Ok(Bounce::Continue { expr: body, env, cont });
                }
                Value::Cons { head: first_binding, tail: rest_bindings } => {
                    let first_binding = first_binding;
                    let rest_bindings = rest_bindings;
                    // first_binding = Cons(name, Cons(init_expr, Nil))  or Cons(name, init_expr)
                    let bname = cons_head(arena, first_binding);
                    let init_expr = cons_head(arena, cons_tail(arena, first_binding));
                    let data = pack_cont_let_data(arena, bname, rest_bindings, body, env);
                    let let_cont = arena.alloc(Value::ContLet { data, next: cont }).unwrap();
                    return Ok(Bounce::Continue { expr: init_expr, env, cont: let_cont });
                }
                _ => return Err(EvalError::new(ErrorKind::Malformed)),
            }
        }
        if sym_eq(arena, s, "match") {
            // (match <expr> (<pattern> <body>) ...)
            let scrutinee = cons_head(arena, tail);
            let clauses = cons_tail(arena, tail);
            let data = pack_cont_match_data(arena, clauses, env);
            let match_cont = arena.alloc(Value::ContMatch { data, next: cont }).unwrap();
            // Force the scrutinee
            return Ok(Bounce::Continue { expr: scrutinee, env, cont: match_cont });
        }
    }

    // Not a special form → function application
    // Evaluate the callee first
    let data = pack_cont_eval_callee_data(arena, tail, env);
    let callee_cont = arena.alloc(Value::ContEvalCallee { data, next: cont }).unwrap();
    Ok(Bounce::Continue { expr: head, env, cont: callee_cont })
}

// ============================================================================
// Function application
// ============================================================================

/// Apply a function (closure or primitive) to arguments.
fn apply_function(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    func: ArenaIndex,
    args: ArenaIndex,
    cont: ArenaIndex,
) -> Result<Bounce, EvalError> {
    match arena.get(func).unwrap() {
        Value::Function { data, .. } => {
            let fd = unpack_function(arena, data);
            let new_env = extend_env(arena, fd.env, fd.params, args);
            Ok(Bounce::Continue { expr: fd.body, env: new_env, cont })
        }
        Value::Primitive { id, .. } => {
            let result = apply_primitive(arena, id, args)?;
            apply_cont(arena, cont, result)
        }
        _ => Err(EvalError::new(ErrorKind::TypeError)),
    }
}

/// Begin strict evaluation of arguments for an impure function call.
fn begin_strict_eval(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    func: ArenaIndex,
    arg_exprs: ArenaIndex,
    env: ArenaIndex,
    cont: ArenaIndex,
) -> Result<Bounce, EvalError> {
    match arena.get(arg_exprs).unwrap() {
        Value::Nil => {
            let nil = arena.alloc(Value::Nil).unwrap();
            apply_function(arena, func, nil, cont)
        }
        Value::Cons { head: first, tail: rest } => {
            let first = first; let rest = rest;
            let nil = arena.alloc(Value::Nil).unwrap();
            let data = pack_eval_arg_data(arena, func, nil, rest, env);
            let eval_cont = arena.alloc(Value::ContEvalArg { data, next: cont }).unwrap();
            Ok(Bounce::Continue { expr: first, env, cont: eval_cont })
        }
        _ => Err(EvalError::new(ErrorKind::Malformed)),
    }
}

// ============================================================================
// Pattern matching
// ============================================================================

/// Try to match `value` against each clause in a cons-list of
/// `(pattern body)` pairs.
fn try_match_clauses(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    value: ArenaIndex,
    clauses: ArenaIndex,
    env: ArenaIndex,
    cont: ArenaIndex,
) -> Result<Bounce, EvalError> {
    let mut cur = clauses;
    loop {
        match arena.get(cur).unwrap() {
            Value::Cons { head: clause, tail: rest } => {
                let clause = clause; let rest = rest;
                let pattern = cons_head(arena, clause);
                let body = cons_head(arena, cons_tail(arena, clause));

                if let Some(new_env) = try_match_pattern(arena, value, pattern, env) {
                    return Ok(Bounce::Continue { expr: body, env: new_env, cont });
                }
                cur = rest;
            }
            _ => return Err(EvalError::new(ErrorKind::MatchFailure)),
        }
    }
}

/// Try to match `value` against `pattern`, returning an extended
/// environment on success or `None` on failure.
fn try_match_pattern(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    value: ArenaIndex,
    pattern: ArenaIndex,
    env: ArenaIndex,
) -> Option<ArenaIndex> {
    match arena.get(pattern).unwrap() {
        // Wildcard: _ matches anything
        Value::Symbol(s) if sym_eq(arena, s, "_") => {
            Some(env)
        }
        // Variable binding: any other symbol binds the value
        Value::Symbol(_) => {
            Some(env_define(arena, env, pattern, value))
        }
        // Literal int
        Value::Int(n) => {
            match arena.get(value).unwrap() {
                Value::Int(m) if m == n => Some(env),
                _ => None,
            }
        }
        // Literal bool
        Value::Bool(b) => {
            match arena.get(value).unwrap() {
                Value::Bool(vb) if vb == b => Some(env),
                _ => None,
            }
        }
        // Nil
        Value::Nil => {
            match arena.get(value).unwrap() {
                Value::Nil => Some(env),
                _ => None,
            }
        }
        // Cons pattern: match head and tail
        Value::Cons { head: ph, tail: pt } => {
            let ph = ph; let pt = pt;
            // Check if this is a (quote ...) pattern
            if let Value::Symbol(s) = arena.get(ph).unwrap() {
                if sym_eq(arena, s, "quote") {
                    // Quoted literal — compare structurally
                    let datum = cons_head(arena, pt);
                    return if structural_eq(arena, value, datum) { Some(env) } else { None };
                }
                if sym_eq(arena, s, "cons") {
                    // (cons <ph> <pt>) pattern
                    let head_pat = cons_head(arena, pt);
                    let tail_pat = cons_head(arena, cons_tail(arena, pt));
                    match arena.get(value).unwrap() {
                        Value::Cons { head: vh, tail: vt } => {
                            let vh = vh; let vt = vt;
                            let env2 = try_match_pattern(arena, vh, head_pat, env)?;
                            try_match_pattern(arena, vt, tail_pat, env2)
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Structural equality for quoted data.
fn structural_eq(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    a: ArenaIndex,
    b: ArenaIndex,
) -> bool {
    match (arena.get(a).unwrap(), arena.get(b).unwrap()) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Nil, Value::Nil) => true,
        (Value::Symbol(x), Value::Symbol(y)) => x == y,
        (Value::Cons { head: ah, tail: at }, Value::Cons { head: bh, tail: bt }) => {
            structural_eq(arena, ah, bh) && structural_eq(arena, at, bt)
        }
        _ => false,
    }
}

// ============================================================================
// Top-level sequence evaluation
// ============================================================================

/// Evaluate a sequence of top-level expressions, threading the
/// environment through `def` forms.
///
/// Returns `(result, final_env)`.
pub fn eval_sequence(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    exprs: ArenaIndex,
    env: ArenaIndex,
) -> Result<(ArenaIndex, ArenaIndex), EvalError> {
    let mut cur = exprs;
    let mut current_env = env;
    let mut last_result = arena.alloc(Value::Nil).unwrap();

    loop {
        match arena.get(cur).unwrap() {
            Value::Nil => return Ok((last_result, current_env)),
            Value::Cons { head: expr, tail: rest } => {
                let expr = expr; let rest = rest;

                // Check if this is a def form
                if is_def_form(arena, expr) {
                    // For def, the result is the new environment
                    let result = run(arena, expr, current_env)?;
                    // Result of def is the new env
                    match arena.get(result).unwrap() {
                        Value::EnvFrame { .. } => {
                            current_env = result;
                        }
                        _ => {
                            // def didn't return an env — shouldn't happen
                            last_result = result;
                        }
                    }
                } else {
                    last_result = run(arena, expr, current_env)?;
                }
                cur = rest;
            }
            _ => return Err(EvalError::new(ErrorKind::Malformed)),
        }
    }
}

/// Check if an expression is a `def` form.
fn is_def_form(arena: &Arena<Value, { crate::ARENA_SIZE }>, expr: ArenaIndex) -> bool {
    if let Value::Cons { head, .. } = arena.get(expr).unwrap() {
        if let Value::Symbol(s) = arena.get(head).unwrap() {
            return sym_eq(arena, s, "def");
        }
    }
    false
}
