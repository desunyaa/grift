//! Built-in primitive operations.
//!
//! Each primitive forces its operands as needed, computes the result,
//! and returns a freshly allocated `Value` in the arena.

use grift_arena::{Arena, ArenaIndex};
use crate::value::{Value, PrimId};
use crate::error::{EvalError, ErrorKind};

/// Force an arena value to an integer, following `Evaluated` and
/// `Thunk` chains until we reach an `Int`.
///
/// Note: this is a *synchronous* force that only follows already-
/// evaluated thunks.  The trampoline is responsible for ensuring
/// arguments are fully forced before calling primitives.
fn force_int(arena: &Arena<Value, { crate::ARENA_SIZE }>, idx: ArenaIndex) -> Result<i64, EvalError> {
    let mut cur = idx;
    loop {
        match arena.get(cur).unwrap() {
            Value::Int(n) => return Ok(n),
            Value::Evaluated { value } => cur = value,
            _ => return Err(EvalError::new(ErrorKind::TypeError)),
        }
    }
}

/// Force to a boolean.
fn force_bool(arena: &Arena<Value, { crate::ARENA_SIZE }>, idx: ArenaIndex) -> Result<bool, EvalError> {
    let mut cur = idx;
    loop {
        match arena.get(cur).unwrap() {
            Value::Bool(b) => return Ok(b),
            Value::Evaluated { value } => cur = value,
            _ => return Err(EvalError::new(ErrorKind::TypeError)),
        }
    }
}

/// Force to a cons cell, returning `(head, tail)`.
fn force_cons(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    idx: ArenaIndex,
) -> Result<(ArenaIndex, ArenaIndex), EvalError> {
    let mut cur = idx;
    loop {
        match arena.get(cur).unwrap() {
            Value::Cons { head, tail } => return Ok((head, tail)),
            Value::Evaluated { value } => cur = value,
            _ => return Err(EvalError::new(ErrorKind::TypeError)),
        }
    }
}

/// Force a value, following Evaluated chains.
fn force_val(arena: &Arena<Value, { crate::ARENA_SIZE }>, idx: ArenaIndex) -> ArenaIndex {
    let mut cur = idx;
    loop {
        match arena.get(cur).unwrap() {
            Value::Evaluated { value } => cur = value,
            _ => return cur,
        }
    }
}

/// Get the nth argument from a cons-list of evaluated arguments.
fn nth_arg(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    args: ArenaIndex,
    n: usize,
) -> Result<ArenaIndex, EvalError> {
    let mut cur = args;
    for _ in 0..n {
        match arena.get(cur).unwrap() {
            Value::Cons { tail, .. } => cur = tail,
            _ => return Err(EvalError::new(ErrorKind::WrongArgCount)),
        }
    }
    match arena.get(cur).unwrap() {
        Value::Cons { head, .. } => Ok(head),
        _ => Err(EvalError::new(ErrorKind::WrongArgCount)),
    }
}

/// Apply a primitive operation to its (already-forced for impure,
/// possibly thunked for pure) arguments.
///
/// Pure primitives that need concrete values (arithmetic, comparisons)
/// force through `Evaluated` redirects but NOT through `Thunk`
/// (the trampoline guarantees thunks are resolved before we get here
/// for primitives that need concrete values).
pub fn apply_primitive(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    id: PrimId,
    args: ArenaIndex,
) -> Result<ArenaIndex, EvalError> {
    match id {
        PrimId::Add => {
            let a = force_int(arena, nth_arg(arena, args, 0)?)?;
            let b = force_int(arena, nth_arg(arena, args, 1)?)?;
            Ok(arena.alloc(Value::Int(a.wrapping_add(b))).unwrap())
        }
        PrimId::Sub => {
            let a = force_int(arena, nth_arg(arena, args, 0)?)?;
            let b = force_int(arena, nth_arg(arena, args, 1)?)?;
            Ok(arena.alloc(Value::Int(a.wrapping_sub(b))).unwrap())
        }
        PrimId::Mul => {
            let a = force_int(arena, nth_arg(arena, args, 0)?)?;
            let b = force_int(arena, nth_arg(arena, args, 1)?)?;
            Ok(arena.alloc(Value::Int(a.wrapping_mul(b))).unwrap())
        }
        PrimId::Div => {
            let a = force_int(arena, nth_arg(arena, args, 0)?)?;
            let b = force_int(arena, nth_arg(arena, args, 1)?)?;
            if b == 0 {
                return Err(EvalError::new(ErrorKind::DivisionByZero));
            }
            Ok(arena.alloc(Value::Int(a / b)).unwrap())
        }
        PrimId::Mod => {
            let a = force_int(arena, nth_arg(arena, args, 0)?)?;
            let b = force_int(arena, nth_arg(arena, args, 1)?)?;
            if b == 0 {
                return Err(EvalError::new(ErrorKind::DivisionByZero));
            }
            Ok(arena.alloc(Value::Int(a % b)).unwrap())
        }
        PrimId::Eq => {
            let a = force_int(arena, nth_arg(arena, args, 0)?)?;
            let b = force_int(arena, nth_arg(arena, args, 1)?)?;
            Ok(arena.alloc(Value::Bool(a == b)).unwrap())
        }
        PrimId::Lt => {
            let a = force_int(arena, nth_arg(arena, args, 0)?)?;
            let b = force_int(arena, nth_arg(arena, args, 1)?)?;
            Ok(arena.alloc(Value::Bool(a < b)).unwrap())
        }
        PrimId::Gt => {
            let a = force_int(arena, nth_arg(arena, args, 0)?)?;
            let b = force_int(arena, nth_arg(arena, args, 1)?)?;
            Ok(arena.alloc(Value::Bool(a > b)).unwrap())
        }
        PrimId::Lte => {
            let a = force_int(arena, nth_arg(arena, args, 0)?)?;
            let b = force_int(arena, nth_arg(arena, args, 1)?)?;
            Ok(arena.alloc(Value::Bool(a <= b)).unwrap())
        }
        PrimId::Gte => {
            let a = force_int(arena, nth_arg(arena, args, 0)?)?;
            let b = force_int(arena, nth_arg(arena, args, 1)?)?;
            Ok(arena.alloc(Value::Bool(a >= b)).unwrap())
        }
        PrimId::ConsP => {
            let h = nth_arg(arena, args, 0)?;
            let t = nth_arg(arena, args, 1)?;
            // cons does NOT force arguments
            Ok(arena.alloc(Value::Cons { head: h, tail: t }).unwrap())
        }
        PrimId::Car => {
            let pair_idx = nth_arg(arena, args, 0)?;
            let (head, _) = force_cons(arena, pair_idx)?;
            // Return head index without forcing
            Ok(head)
        }
        PrimId::Cdr => {
            let pair_idx = nth_arg(arena, args, 0)?;
            let (_, tail) = force_cons(arena, pair_idx)?;
            // Return tail index without forcing
            Ok(tail)
        }
        PrimId::IsNull => {
            let v = force_val(arena, nth_arg(arena, args, 0)?);
            let is_nil = matches!(arena.get(v).unwrap(), Value::Nil);
            Ok(arena.alloc(Value::Bool(is_nil)).unwrap())
        }
        PrimId::IsPair => {
            let v = force_val(arena, nth_arg(arena, args, 0)?);
            let is_pair = matches!(arena.get(v).unwrap(), Value::Cons { .. });
            Ok(arena.alloc(Value::Bool(is_pair)).unwrap())
        }
        PrimId::Not => {
            let b = force_bool(arena, nth_arg(arena, args, 0)?)?;
            Ok(arena.alloc(Value::Bool(!b)).unwrap())
        }
        PrimId::Show => {
            // In no_std, show just returns the value as-is
            let v = nth_arg(arena, args, 0)?;
            Ok(force_val(arena, v))
        }
    }
}
