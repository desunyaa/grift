//! Environment operations.
//!
//! Environments are linked lists of `EnvFrame` values in the arena.
//! Each frame holds a cons-list of `Cons(symbol, value)` bindings and
//! a pointer to the parent frame.

use grift_arena::{Arena, ArenaIndex};
use crate::value::{Value, SymbolId};
use crate::error::{EvalError, ErrorKind};
use crate::intern::sym_ids_eq;

/// Look up a symbol in the environment chain.
pub fn env_lookup(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    env: ArenaIndex,
    sym: SymbolId,
) -> Result<ArenaIndex, EvalError> {
    let mut cur_env = env;
    loop {
        match arena.get(cur_env).unwrap() {
            Value::EnvFrame { bindings, parent } => {
                let bindings = bindings;
                let parent = parent;
                if let Some(val) = alist_lookup(arena, bindings, sym) {
                    return Ok(val);
                }
                match arena.get(parent).unwrap() {
                    Value::Nil => return Err(EvalError::new(ErrorKind::UnboundVariable)),
                    _ => cur_env = parent,
                }
            }
            Value::Nil => return Err(EvalError::new(ErrorKind::UnboundVariable)),
            _ => return Err(EvalError::new(ErrorKind::Internal)),
        }
    }
}

/// Search an association list (cons list of `Cons(key, value)` pairs)
/// for a matching symbol.
pub fn alist_lookup(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    list: ArenaIndex,
    sym: SymbolId,
) -> Option<ArenaIndex> {
    let mut cur = list;
    loop {
        match arena.get(cur).unwrap() {
            Value::Cons { head: pair, tail: rest } => {
                let pair = pair;
                let rest = rest;
                match arena.get(pair).unwrap() {
                    Value::Cons { head: key, tail: val } => {
                        let key = key;
                        let val = val;
                        if let Value::Symbol(s) = arena.get(key).unwrap() {
                            if sym_ids_eq(arena, s, sym) {
                                return Some(val);
                            }
                        }
                        cur = rest;
                    }
                    _ => { cur = rest; }
                }
            }
            _ => return None,
        }
    }
}

/// Extend an environment with new bindings.
///
/// `params` and `args` are parallel cons lists of symbols and values.
pub fn extend_env(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    parent: ArenaIndex,
    params: ArenaIndex,
    args: ArenaIndex,
) -> ArenaIndex {
    let bindings = zip_to_alist(arena, params, args);
    arena.alloc(Value::EnvFrame { bindings, parent }).unwrap()
}

/// Zip two parallel cons lists into an association list.
pub fn zip_to_alist(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    keys: ArenaIndex,
    vals: ArenaIndex,
) -> ArenaIndex {
    let mut k_cur = keys;
    let mut v_cur = vals;
    let mut result = arena.alloc(Value::Nil).unwrap();

    // Build in reverse then reverse at the end, OR build forwards via a
    // simple iterative approach.  Since order matters for lookup (first
    // match wins), we build the list by prepending and then rely on the
    // fact that lookup searches from the head — which matches the
    // left-to-right order of params/args already.
    //
    // Actually, to keep correct ordering we build by consing in reverse
    // order.  But we can also just prepend and accept LIFO ordering for
    // the flat alist.  Since each binding is unique per scope, order in
    // the alist does not affect correctness.

    loop {
        let k_val = arena.get(k_cur).unwrap();
        let v_val = arena.get(v_cur).unwrap();
        match (k_val, v_val) {
            (Value::Cons { head: k, tail: ks }, Value::Cons { head: v, tail: vs }) => {
                let k = k; let ks = ks; let v = v; let vs = vs;
                let pair = arena.alloc(Value::Cons { head: k, tail: v }).unwrap();
                result = arena.alloc(Value::Cons { head: pair, tail: result }).unwrap();
                k_cur = ks;
                v_cur = vs;
            }
            _ => break,
        }
    }
    result
}

/// Add a single binding to an environment, returning the new environment.
///
/// Creates a new frame with the single binding, whose parent is the
/// given `env`.  This ensures all prior bindings remain accessible.
pub fn env_define(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    env: ArenaIndex,
    name: ArenaIndex,
    value: ArenaIndex,
) -> ArenaIndex {
    let nil = arena.alloc(Value::Nil).unwrap();
    let pair = arena.alloc(Value::Cons { head: name, tail: value }).unwrap();
    let bindings = arena.alloc(Value::Cons { head: pair, tail: nil }).unwrap();
    arena.alloc(Value::EnvFrame { bindings, parent: env }).unwrap()
}
