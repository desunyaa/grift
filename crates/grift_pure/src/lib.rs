#![no_std]
#![forbid(unsafe_code)]

//! # grift_pure — Minimal Lazy Functional Lisp
//!
//! A tree-walking interpreter for a minimal, purely functional Lisp
//! with **call-by-need (lazy) evaluation** for pure functions and
//! **call-by-value (strict) evaluation** for impure functions.
//!
//! ## Design constraints
//!
//! - `no_std`, `no_alloc`, `no_unsafe`
//! - Every [`Value`] variant holds at most two [`ArenaIndex`] fields
//! - All runtime data lives in a [`grift_arena::Arena`]
//! - Continuation-passing style (CPS) with a trampoline loop
//!
//! ## Core forms
//!
//! ```text
//! (quote <datum>)
//! (if <test> <then> <else>)
//! (fn (<params...>) <body>)
//! (def <name> <expr>)
//! (def (<name> <params...>) <body>)
//! (do <expr1> ... <exprN>)
//! (let ((<name> <expr>) ...) <body>)
//! (match <expr> (<pattern> <body>) ...)
//! ```

pub use grift_arena::{Arena, ArenaIndex};

/// Default arena size for the pure evaluator.
pub const ARENA_SIZE: usize = 65536;

pub mod value;
pub mod pack;
pub mod env;
pub mod thunk;
pub mod purity;
pub mod primitives;
pub mod error;
pub mod intern;
pub mod eval;

pub use value::{Value, Purity, PrimId, SymbolId};
pub use error::{EvalError, ErrorKind};

/// Create a new arena pre-populated with `Nil` in slot 0.
pub fn new_arena() -> Arena<Value, ARENA_SIZE> {
    let arena = Arena::new(Value::Nil);
    // Reserve slot 0 for Nil
    let _ = arena.alloc(Value::Nil);
    arena
}

/// Create a default global environment with built-in primitives.
pub fn default_env(arena: &Arena<Value, ARENA_SIZE>) -> ArenaIndex {
    let nil = arena.alloc(Value::Nil).unwrap();
    let mut bindings = nil;

    // Helper: bind a primitive
    let mut bind = |name: &str, id: PrimId| {
        let purity = id.purity();
        let sym = intern::intern(arena, name);
        let sym_val = arena.alloc(Value::Symbol(sym)).unwrap();
        let prim = arena.alloc(Value::Primitive { id, purity }).unwrap();
        let pair = arena.alloc(Value::Cons { head: sym_val, tail: prim }).unwrap();
        bindings = arena.alloc(Value::Cons { head: pair, tail: bindings }).unwrap();
    };

    bind("+", PrimId::Add);
    bind("-", PrimId::Sub);
    bind("*", PrimId::Mul);
    bind("/", PrimId::Div);
    bind("%", PrimId::Mod);
    bind("=", PrimId::Eq);
    bind("<", PrimId::Lt);
    bind(">", PrimId::Gt);
    bind("<=", PrimId::Lte);
    bind(">=", PrimId::Gte);
    bind("cons", PrimId::ConsP);
    bind("car", PrimId::Car);
    bind("cdr", PrimId::Cdr);
    bind("null?", PrimId::IsNull);
    bind("pair?", PrimId::IsPair);
    bind("not", PrimId::Not);
    bind("show", PrimId::Show);

    let parent = arena.alloc(Value::Nil).unwrap();
    arena.alloc(Value::EnvFrame { bindings, parent }).unwrap()
}
