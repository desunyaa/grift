//! Core value types for the pure lazy evaluator.
//!
//! Every variant of [`Value`] has at most two [`ArenaIndex`] fields.
//! Extra data is packed into arena-allocated cons chains.

use grift_arena::{ArenaIndex, Trace};

// ============================================================================
// SymbolId — lightweight interned symbol identifier
// ============================================================================

/// Interned symbol identifier.
///
/// Symbols are interned as contiguous character sequences in the arena.
/// A `SymbolId` is simply the [`ArenaIndex`] that points to the start of
/// the interned string data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymbolId(pub ArenaIndex);

// ============================================================================
// Purity tag
// ============================================================================

/// Purity tag for functions.
///
/// Pure functions use call-by-need (lazy) argument evaluation.
/// Impure functions use call-by-value (strict) argument evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purity {
    Pure,
    Impure,
}

// ============================================================================
// PrimId — built-in primitive identifier
// ============================================================================

/// Identifies a built-in primitive operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimId {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Lt,
    Gt,
    Lte,
    Gte,
    ConsP,
    Car,
    Cdr,
    IsNull,
    IsPair,
    Not,
    Show,
}

impl PrimId {
    /// The purity of this primitive.
    pub fn purity(self) -> Purity {
        // All current primitives are pure (no IO in no_std)
        match self {
            _ => Purity::Pure,
        }
    }
}

// ============================================================================
// Value enum
// ============================================================================

/// Runtime value for the pure lazy evaluator.
///
/// # Invariant
///
/// Every variant contains **at most two [`ArenaIndex`]** fields.
/// Variants that logically need more data pack the extras into
/// arena-allocated `Cons` chains pointed to by a single index.
#[derive(Clone, Copy, Debug)]
pub enum Value {
    // === Immediates (zero ArenaIndex) ===
    /// Integer literal.
    Int(i64),
    /// Boolean literal.
    Bool(bool),
    /// Nil / empty list.
    Nil,

    // === One ArenaIndex ===
    /// Interned symbol.
    Symbol(SymbolId),
    /// Forced thunk — redirects to the computed value.
    Evaluated { value: ArenaIndex },

    // === Two ArenaIndex ===
    /// Cons cell (pair).
    Cons { head: ArenaIndex, tail: ArenaIndex },

    /// Lazy thunk: unevaluated expression + captured environment.
    Thunk { expr: ArenaIndex, env: ArenaIndex },

    /// Function closure.
    ///
    /// `data` → `Cons(params, Cons(body, env))`
    Function { data: ArenaIndex, purity: Purity },

    /// Built-in primitive (no ArenaIndex needed).
    Primitive { id: PrimId, purity: Purity },

    /// Environment frame.
    ///
    /// `bindings` = cons list of `Cons(symbol, value)` pairs.
    /// `parent`   = parent `EnvFrame` or `Nil`.
    EnvFrame { bindings: ArenaIndex, parent: ArenaIndex },

    // === Continuations (all ≤ 2 ArenaIndex: data + next) ===
    /// Halt continuation — evaluation is done.
    ContHalt,

    /// Force continuation — write result back into thunk slot.
    ContForce { slot: ArenaIndex, next: ArenaIndex },

    /// If continuation — pick branch based on forced condition.
    ///
    /// `data` → `Cons(then_expr, Cons(else_expr, env))`
    ContIf { data: ArenaIndex, next: ArenaIndex },

    /// Callee-evaluated continuation — function position evaluated, handle args.
    ///
    /// `data` → `Cons(arg_exprs, env)`
    ContEvalCallee { data: ArenaIndex, next: ArenaIndex },

    /// Strict arg evaluation continuation (impure call path).
    ///
    /// `data` → `Cons(func, Cons(evaluated_args, Cons(remaining_args, env)))`
    ContEvalArg { data: ArenaIndex, next: ArenaIndex },

    /// Do-block sequencing continuation.
    ///
    /// `data` → `Cons(remaining_exprs, env)`
    ContDo { data: ArenaIndex, next: ArenaIndex },

    /// Let-binding continuation.
    ///
    /// `data` → `Cons(name_symbol, Cons(remaining_bindings, Cons(body, env)))`
    ContLet { data: ArenaIndex, next: ArenaIndex },

    /// Match continuation.
    ///
    /// `data` → `Cons(clauses, env)`
    ContMatch { data: ArenaIndex, next: ArenaIndex },

    /// Def continuation — bind a value in the current environment.
    ///
    /// `data` → `Cons(name_symbol, env)`
    ContDef { data: ArenaIndex, next: ArenaIndex },
}

// ============================================================================
// GC tracing
// ============================================================================

impl<const N: usize> Trace<Value, N> for Value {
    fn trace<F: FnMut(ArenaIndex)>(&self, mut tracer: F) {
        match *self {
            // Zero ArenaIndex
            Value::Int(_) | Value::Bool(_) | Value::Nil => {}
            Value::Primitive { .. } => {}
            Value::ContHalt => {}

            // One ArenaIndex
            Value::Symbol(SymbolId(idx)) => tracer(idx),
            Value::Evaluated { value } => tracer(value),

            // Two ArenaIndex
            Value::Cons { head, tail } => { tracer(head); tracer(tail); }
            Value::Thunk { expr, env } => { tracer(expr); tracer(env); }
            Value::Function { data, .. } => tracer(data),
            Value::EnvFrame { bindings, parent } => { tracer(bindings); tracer(parent); }

            // Continuations — all have data + next (or slot + next)
            Value::ContForce { slot, next } => { tracer(slot); tracer(next); }
            Value::ContIf { data, next } => { tracer(data); tracer(next); }
            Value::ContEvalCallee { data, next } => { tracer(data); tracer(next); }
            Value::ContEvalArg { data, next } => { tracer(data); tracer(next); }
            Value::ContDo { data, next } => { tracer(data); tracer(next); }
            Value::ContLet { data, next } => { tracer(data); tracer(next); }
            Value::ContMatch { data, next } => { tracer(data); tracer(next); }
            Value::ContDef { data, next } => { tracer(data); tracer(next); }
        }
    }
}
