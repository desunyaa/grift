//! Symbol interning.
//!
//! Symbols are stored as contiguous character data in the arena.
//! A `SymbolId` wraps the `ArenaIndex` of the first character.
//!
//! Layout in arena: `[Int(len), Int(c0), Int(c1), ..., Int(cN-1)]`
//! where each `Int` stores a `char` as its `i64` value.

use grift_arena::{Arena, ArenaIndex};
use crate::value::{Value, SymbolId};

/// Intern a symbol string, returning a `SymbolId`.
///
/// This always allocates a fresh copy.  For a production implementation
/// you would maintain an intern table, but for simplicity we skip
/// deduplication here (the evaluator compares by `SymbolId` equality,
/// so we must ensure the same source name always resolves to the same
/// `SymbolId` — see `intern_or_find`).
pub fn intern(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    name: &str,
) -> SymbolId {
    // Store length then characters
    let len = name.len();
    let start = arena.alloc(Value::Int(len as i64)).unwrap();
    for ch in name.chars() {
        arena.alloc(Value::Int(ch as i64)).unwrap();
    }
    SymbolId(start)
}

/// Compare a `SymbolId` to a string.
pub fn sym_eq(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    sym: SymbolId,
    name: &str,
) -> bool {
    let start = sym.0;
    let len = match arena.get(start).unwrap() {
        Value::Int(n) => n as usize,
        _ => return false,
    };
    if len != name.len() {
        return false;
    }
    let mut idx = start.raw() + 1;
    for ch in name.chars() {
        match arena.get(ArenaIndex::new(idx)).unwrap() {
            Value::Int(c) if c == ch as i64 => {}
            _ => return false,
        }
        idx += 1;
    }
    true
}

/// Get the length of an interned symbol.
pub fn sym_len(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    sym: SymbolId,
) -> usize {
    match arena.get(sym.0).unwrap() {
        Value::Int(n) => n as usize,
        _ => 0,
    }
}

/// Compare two `SymbolId` values by their interned string content.
pub fn sym_ids_eq(
    arena: &Arena<Value, { crate::ARENA_SIZE }>,
    a: SymbolId,
    b: SymbolId,
) -> bool {
    // Fast path: same arena index
    if a.0 == b.0 {
        return true;
    }
    let len_a = sym_len(arena, a);
    let len_b = sym_len(arena, b);
    if len_a != len_b {
        return false;
    }
    for i in 1..=len_a {
        let ca = arena.get(ArenaIndex::new(a.0.raw() + i)).unwrap();
        let cb = arena.get(ArenaIndex::new(b.0.raw() + i)).unwrap();
        match (ca, cb) {
            (Value::Int(x), Value::Int(y)) if x == y => {}
            _ => return false,
        }
    }
    true
}
