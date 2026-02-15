//! Purity analysis.
//!
//! A simple recursive walk that classifies an expression as `Pure` or
//! `Impure`.  Presence of `do` anywhere in the body, or a call to a
//! known-impure binding, makes the function impure.

use grift_arena::{Arena, ArenaIndex};
use crate::value::{Value, Purity, SymbolId};
use crate::env::alist_lookup;
use crate::intern::sym_eq;

/// Analyse the purity of an expression.
///
/// Returns `Impure` if the expression contains a `do` form or calls
/// a known-impure function.  Otherwise returns `Pure`.
pub fn analyze_purity(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    expr: ArenaIndex,
    env: ArenaIndex,
) -> Purity {
    match arena.get(expr).unwrap() {
        Value::Symbol(s) => {
            if let Some(val) = lookup_purity(arena, env, s) {
                if val == Purity::Impure {
                    return Purity::Impure;
                }
            }
            Purity::Pure
        }
        Value::Cons { head, tail } => {
            let head = head;
            let tail = tail;
            // `(do ...)` is always impure
            if is_symbol_named(arena, head, "do") {
                return Purity::Impure;
            }
            // Check callee
            if analyze_purity(arena, head, env) == Purity::Impure {
                return Purity::Impure;
            }
            // Check arguments
            let mut cursor = tail;
            loop {
                match arena.get(cursor).unwrap() {
                    Value::Cons { head: arg, tail: rest } => {
                        let arg = arg;
                        let rest = rest;
                        if analyze_purity(arena, arg, env) == Purity::Impure {
                            return Purity::Impure;
                        }
                        cursor = rest;
                    }
                    _ => break,
                }
            }
            Purity::Pure
        }
        _ => Purity::Pure,
    }
}

/// Check whether `idx` points to a `Symbol` whose interned name matches
/// the given string.
fn is_symbol_named(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    idx: ArenaIndex,
    name: &str,
) -> bool {
    if let Value::Symbol(s) = arena.get(idx).unwrap() {
        sym_eq(arena, s, name)
    } else {
        false
    }
}

/// Look up the purity of a symbol in the environment.
///
/// If the symbol resolves to a `Function` or `Primitive`, returns its
/// purity tag.  Otherwise returns `None`.
fn lookup_purity(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    env: ArenaIndex,
    sym: SymbolId,
) -> Option<Purity> {
    // Walk env chain looking for the binding
    let mut cur_env = env;
    loop {
        match arena.get(cur_env).unwrap() {
            Value::EnvFrame { bindings, parent } => {
                let bindings = bindings;
                let parent = parent;
                if let Some(val_idx) = alist_lookup(arena, bindings, sym) {
                    match arena.get(val_idx).unwrap() {
                        Value::Function { purity, .. } => return Some(purity),
                        Value::Primitive { purity, .. } => return Some(purity),
                        _ => return None,
                    }
                }
                match arena.get(parent).unwrap() {
                    Value::Nil => return None,
                    _ => cur_env = parent,
                }
            }
            _ => return None,
        }
    }
}
