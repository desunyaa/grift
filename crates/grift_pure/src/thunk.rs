//! Thunk utilities.
//!
//! Helpers for wrapping argument lists in thunks (lazy path) and for
//! reversing cons lists (used by the strict-arg accumulator).

use grift_arena::{Arena, ArenaIndex};
use crate::value::Value;

/// Wrap each element of an arg-expression cons list in a `Thunk`,
/// producing a new cons list of thunks.
pub fn thunk_arg_list(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    arg_exprs: ArenaIndex,
    env: ArenaIndex,
) -> ArenaIndex {
    let mut cur = arg_exprs;
    // Build the thunked list in reverse, then reverse it.
    let mut acc = arena.alloc(Value::Nil).unwrap();
    loop {
        match arena.get(cur).unwrap() {
            Value::Cons { head: expr, tail: rest } => {
                let expr = expr;
                let rest = rest;
                let thunk = arena.alloc(Value::Thunk { expr, env }).unwrap();
                acc = arena.alloc(Value::Cons { head: thunk, tail: acc }).unwrap();
                cur = rest;
            }
            _ => break,
        }
    }
    reverse_cons_list(arena, acc)
}

/// Reverse a cons list, returning a new cons list.
pub fn reverse_cons_list(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    list: ArenaIndex,
) -> ArenaIndex {
    let mut acc = arena.alloc(Value::Nil).unwrap();
    let mut cur = list;
    loop {
        match arena.get(cur).unwrap() {
            Value::Cons { head, tail } => {
                let head = head;
                let tail = tail;
                acc = arena.alloc(Value::Cons { head, tail: acc }).unwrap();
                cur = tail;
            }
            _ => return acc,
        }
    }
}
