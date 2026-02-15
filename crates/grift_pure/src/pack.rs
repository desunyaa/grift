//! Pack / unpack helpers for multi-field variants.
//!
//! Every continuation and `Function` variant that logically needs more
//! than two arena references packs the extras into a cons chain stored
//! in the arena.  **Only this module knows the layout.**  All other code
//! calls `pack_*` / `unpack_*`.

use grift_arena::{Arena, ArenaIndex};
use crate::value::Value;

// ============================================================================
// Cons-cell accessors
// ============================================================================

/// Head of a cons cell.  Panics (via `.unwrap()`) if the index does not
/// point to a `Cons`.
pub fn cons_head(arena: &Arena<Value, { crate::ARENA_SIZE }>, idx: ArenaIndex) -> ArenaIndex {
    match arena.get(idx).unwrap() {
        Value::Cons { head, .. } => head,
        _ => ArenaIndex::NIL,
    }
}

/// Tail of a cons cell.
pub fn cons_tail(arena: &Arena<Value, { crate::ARENA_SIZE }>, idx: ArenaIndex) -> ArenaIndex {
    match arena.get(idx).unwrap() {
        Value::Cons { tail, .. } => tail,
        _ => ArenaIndex::NIL,
    }
}

// ============================================================================
// Function
// ============================================================================

/// Unpacked function data.
#[derive(Clone, Copy)]
pub struct FunctionData {
    pub params: ArenaIndex,
    pub body: ArenaIndex,
    pub env: ArenaIndex,
}

/// Pack function closure data: `Cons(params, Cons(body, env))`.
pub fn pack_function_data(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    params: ArenaIndex,
    body: ArenaIndex,
    env: ArenaIndex,
) -> ArenaIndex {
    let inner = arena.alloc(Value::Cons { head: body, tail: env }).unwrap();
    arena.alloc(Value::Cons { head: params, tail: inner }).unwrap()
}

/// Unpack function closure data.
pub fn unpack_function(arena: &Arena<Value, { crate::ARENA_SIZE }>, data: ArenaIndex) -> FunctionData {
    let params = cons_head(arena, data);
    let rest = cons_tail(arena, data);
    let body = cons_head(arena, rest);
    let env = cons_tail(arena, rest);
    FunctionData { params, body, env }
}

// ============================================================================
// ContIf
// ============================================================================

/// Unpacked ContIf data.
#[derive(Clone, Copy)]
pub struct ContIfData {
    pub then_expr: ArenaIndex,
    pub else_expr: ArenaIndex,
    pub env: ArenaIndex,
}

/// Pack ContIf data: `Cons(then_expr, Cons(else_expr, env))`.
pub fn pack_cont_if_data(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    then_expr: ArenaIndex,
    else_expr: ArenaIndex,
    env: ArenaIndex,
) -> ArenaIndex {
    let inner = arena.alloc(Value::Cons { head: else_expr, tail: env }).unwrap();
    arena.alloc(Value::Cons { head: then_expr, tail: inner }).unwrap()
}

/// Unpack ContIf data.
pub fn unpack_cont_if(arena: &Arena<Value, { crate::ARENA_SIZE }>, data: ArenaIndex) -> ContIfData {
    let then_expr = cons_head(arena, data);
    let rest = cons_tail(arena, data);
    let else_expr = cons_head(arena, rest);
    let env = cons_tail(arena, rest);
    ContIfData { then_expr, else_expr, env }
}

// ============================================================================
// ContEvalCallee
// ============================================================================

/// Unpacked ContEvalCallee data.
#[derive(Clone, Copy)]
pub struct ContEvalCalleeData {
    pub arg_exprs: ArenaIndex,
    pub env: ArenaIndex,
}

/// Pack ContEvalCallee data: `Cons(arg_exprs, env)`.
pub fn pack_cont_eval_callee_data(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    arg_exprs: ArenaIndex,
    env: ArenaIndex,
) -> ArenaIndex {
    arena.alloc(Value::Cons { head: arg_exprs, tail: env }).unwrap()
}

/// Unpack ContEvalCallee data.
pub fn unpack_cont_eval_callee(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    data: ArenaIndex,
) -> ContEvalCalleeData {
    let arg_exprs = cons_head(arena, data);
    let env = cons_tail(arena, data);
    ContEvalCalleeData { arg_exprs, env }
}

// ============================================================================
// ContEvalArg
// ============================================================================

/// Unpacked ContEvalArg data.
#[derive(Clone, Copy)]
pub struct ContEvalArgData {
    pub func: ArenaIndex,
    pub evaluated_args: ArenaIndex,
    pub remaining_args: ArenaIndex,
    pub env: ArenaIndex,
}

/// Pack ContEvalArg data: `Cons(func, Cons(evaluated_args, Cons(remaining_args, env)))`.
pub fn pack_eval_arg_data(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    func: ArenaIndex,
    evaluated_args: ArenaIndex,
    remaining_args: ArenaIndex,
    env: ArenaIndex,
) -> ArenaIndex {
    let inner2 = arena.alloc(Value::Cons { head: remaining_args, tail: env }).unwrap();
    let inner1 = arena.alloc(Value::Cons { head: evaluated_args, tail: inner2 }).unwrap();
    arena.alloc(Value::Cons { head: func, tail: inner1 }).unwrap()
}

/// Unpack ContEvalArg data.
pub fn unpack_cont_eval_arg(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    data: ArenaIndex,
) -> ContEvalArgData {
    let func = cons_head(arena, data);
    let rest1 = cons_tail(arena, data);
    let evaluated_args = cons_head(arena, rest1);
    let rest2 = cons_tail(arena, rest1);
    let remaining_args = cons_head(arena, rest2);
    let env = cons_tail(arena, rest2);
    ContEvalArgData { func, evaluated_args, remaining_args, env }
}

// ============================================================================
// ContDo
// ============================================================================

/// Unpacked ContDo data.
#[derive(Clone, Copy)]
pub struct ContDoData {
    pub remaining: ArenaIndex,
    pub env: ArenaIndex,
}

/// Pack ContDo data: `Cons(remaining_exprs, env)`.
pub fn pack_cont_do_data(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    remaining: ArenaIndex,
    env: ArenaIndex,
) -> ArenaIndex {
    arena.alloc(Value::Cons { head: remaining, tail: env }).unwrap()
}

/// Unpack ContDo data.
pub fn unpack_cont_do(arena: &Arena<Value, { crate::ARENA_SIZE }>, data: ArenaIndex) -> ContDoData {
    let remaining = cons_head(arena, data);
    let env = cons_tail(arena, data);
    ContDoData { remaining, env }
}

// ============================================================================
// ContLet
// ============================================================================

/// Unpacked ContLet data.
#[derive(Clone, Copy)]
pub struct ContLetData {
    pub name: ArenaIndex,
    pub remaining_bindings: ArenaIndex,
    pub body: ArenaIndex,
    pub env: ArenaIndex,
}

/// Pack ContLet data: `Cons(name, Cons(remaining_bindings, Cons(body, env)))`.
pub fn pack_cont_let_data(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    name: ArenaIndex,
    remaining_bindings: ArenaIndex,
    body: ArenaIndex,
    env: ArenaIndex,
) -> ArenaIndex {
    let inner2 = arena.alloc(Value::Cons { head: body, tail: env }).unwrap();
    let inner1 = arena.alloc(Value::Cons { head: remaining_bindings, tail: inner2 }).unwrap();
    arena.alloc(Value::Cons { head: name, tail: inner1 }).unwrap()
}

/// Unpack ContLet data.
pub fn unpack_cont_let(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    data: ArenaIndex,
) -> ContLetData {
    let name = cons_head(arena, data);
    let rest1 = cons_tail(arena, data);
    let remaining_bindings = cons_head(arena, rest1);
    let rest2 = cons_tail(arena, rest1);
    let body = cons_head(arena, rest2);
    let env = cons_tail(arena, rest2);
    ContLetData { name, remaining_bindings, body, env }
}

// ============================================================================
// ContMatch
// ============================================================================

/// Unpacked ContMatch data.
#[derive(Clone, Copy)]
pub struct ContMatchData {
    pub clauses: ArenaIndex,
    pub env: ArenaIndex,
}

/// Pack ContMatch data: `Cons(clauses, env)`.
pub fn pack_cont_match_data(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    clauses: ArenaIndex,
    env: ArenaIndex,
) -> ArenaIndex {
    arena.alloc(Value::Cons { head: clauses, tail: env }).unwrap()
}

/// Unpack ContMatch data.
pub fn unpack_cont_match(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    data: ArenaIndex,
) -> ContMatchData {
    let clauses = cons_head(arena, data);
    let env = cons_tail(arena, data);
    ContMatchData { clauses, env }
}

// ============================================================================
// ContDef
// ============================================================================

/// Unpacked ContDef data.
#[derive(Clone, Copy)]
pub struct ContDefData {
    pub name: ArenaIndex,
    pub env: ArenaIndex,
}

/// Pack ContDef data: `Cons(name_symbol, env)`.
pub fn pack_cont_def_data(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    name: ArenaIndex,
    env: ArenaIndex,
) -> ArenaIndex {
    arena.alloc(Value::Cons { head: name, tail: env }).unwrap()
}

/// Unpack ContDef data.
pub fn unpack_cont_def(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    data: ArenaIndex,
) -> ContDefData {
    let name = cons_head(arena, data);
    let env = cons_tail(arena, data);
    ContDefData { name, env }
}
