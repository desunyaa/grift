//! Lisp value type.

use grift_arena::{ArenaError, ArenaIndex};

/// Type-safe identifier for built-in functions.
///
/// Wraps a `u8`, supporting up to 256 builtins. Generated automatically
/// by [`define_builtins!`] and matched in [`Evaluator::apply_builtin`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct BuiltinId(pub(crate) u8);

/// A Lisp value stored in the arena. Variants can only inline max two arenaindex sized data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    /// The empty list / nil.
    Nil,
    /// The unit type `()`.
    Unit,
    Boolean(bool),
    /// Integer number (isize).
    Number(isize),
    /// A symbol, pointing to a `String` value that holds the name.
    Symbol(ArenaIndex),
    /// A cons cell (pair) with inline car and cdr.
    Cons {
        car: ArenaIndex,
        cdr: ArenaIndex,
    },
    /// A string with inline length and pointer to character data.
    String {
        len: usize,
        data: ArenaIndex,
    },
    /// A character.
    Char(char),
    /// Compound operative (vau closure / fexpr).
    /// Created by `(vau params env-param body)`.
    /// params_envparam = (params . env-param), body_env = (body . closed-env)
    Operative {
        params_envparam: ArenaIndex,
        body_env: ArenaIndex,
    },
    /// Applicative wrapper: evaluates arguments, then calls inner combiner.
    /// Created by `(wrap combiner)`. The inner combiner is any callable:
    /// Operative, Builtin, or even another Applicative.
    Applicative(ArenaIndex),
    /// Rust-native primitive operative.
    /// Always an operative — receives unevaluated args + caller env.
    /// Applicative primitives (like +) are (wrap (Builtin id)) at init time.
    Builtin(BuiltinId),
    /// A first-class environment with lexical parent chain.
    /// `bindings`: alist of (symbol . value) pairs in this frame.
    /// `parents`: cons-list of parent environments, or NIL for top-level.
    Environment {
        bindings: ArenaIndex,
        parents: ArenaIndex,
    },
    /// The inert value, written `#inert`.
    /// Returned by combiners whose primary purpose is side-effect (e.g. `$define!`).
    Inert,
    /// The ignore value, written `#ignore`.
    /// Used specifically for parameter matching in formal parameter trees.
    Ignore,

    // — Typed numeric variants —
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    Usize(usize),
    F32(f32),
    F64(f64),

    // — Compound data types —
    /// A fixed-size array with inline length and pointer to contiguous arena data.
    Array {
        len: usize,
        data: ArenaIndex,
    },
    /// A finite heterogeneous sequence with inline length and pointer to arena data.
    Tuple {
        len: usize,
        data: ArenaIndex,
    },
    /// A dynamically-sized view into a contiguous sequence.
    Slice {
        len: usize,
        data: ArenaIndex,
    },
    /// A raw arena pointer (wraps an ArenaIndex).
    Pointer(ArenaIndex),
    /// A reference to another arena value.
    Reference(ArenaIndex),
}

/// Generate a `Value` accessor that pattern-matches on a variant and
/// returns its inner data, or `Err(TypeError)` on mismatch.
macro_rules! value_accessor {
    ($(#[$m:meta])* $name:ident -> $out:ty, $pat:pat => $expr:expr) => {
        $(#[$m])*
        #[inline]
        pub fn $name(self) -> Result<$out, ArenaError> {
            match self {
                $pat => Ok($expr),
                _ => Err(ArenaError::TypeError),
            }
        }
    };
}

impl Value {
    /// Returns the type name as a static string (for error messages).
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Unit => "unit",
            Value::Boolean(_) => "boolean",
            Value::Number(_) => "number",
            Value::Symbol(_) => "symbol",
            Value::Cons { .. } => "pair",
            Value::String { .. } => "string",
            Value::Char(_) => "char",
            Value::Operative { .. } => "operative",
            Value::Applicative(_) => "applicative",
            Value::Builtin(_) => "builtin",
            Value::Environment { .. } => "environment",
            Value::Inert => "inert",
            Value::Ignore => "ignore",
            Value::I8(_) => "i8",
            Value::I16(_) => "i16",
            Value::I32(_) => "i32",
            Value::I64(_) => "i64",
            Value::I128(_) => "i128",
            Value::U8(_) => "u8",
            Value::U16(_) => "u16",
            Value::U32(_) => "u32",
            Value::U64(_) => "u64",
            Value::U128(_) => "u128",
            Value::Usize(_) => "usize",
            Value::F32(_) => "f32",
            Value::F64(_) => "f64",
            Value::Array { .. } => "array",
            Value::Tuple { .. } => "tuple",
            Value::Slice { .. } => "slice",
            Value::Pointer(_) => "pointer",
            Value::Reference(_) => "reference",
        }
    }

    /// True for self-evaluating forms (literals, closures, builtins).
    ///
    /// Self-evaluating values exclude symbols and cons cells
    /// (which require lookup / dispatch).
    #[inline]
    pub fn is_self_evaluating(self) -> bool {
        !matches!(self, Value::Symbol(_) | Value::Cons { .. })
    }

    /// True for immutable, encapsulated types whose identity is
    /// determined by value rather than arena slot (used by `eq?`).
    #[inline]
    pub fn is_immutable(self) -> bool {
        matches!(
            self,
            Value::Nil
                | Value::Unit
                | Value::Boolean(_)
                | Value::Number(_)
                | Value::Symbol(_)
                | Value::Char(_)
                | Value::Inert
                | Value::Ignore
                | Value::I8(_)
                | Value::I16(_)
                | Value::I32(_)
                | Value::I64(_)
                | Value::I128(_)
                | Value::U8(_)
                | Value::U16(_)
                | Value::U32(_)
                | Value::U64(_)
                | Value::U128(_)
                | Value::Usize(_)
                | Value::F32(_)
                | Value::F64(_)
        )
    }

    value_accessor! {
        /// Extract the numeric value, or `Err(TypeError)` if not a number.
        as_number -> isize, Value::Number(n) => n
    }

    value_accessor! {
        /// Extract the car and cdr of a cons cell.
        as_cons -> (ArenaIndex, ArenaIndex), Value::Cons { car, cdr } => (car, cdr)
    }

    value_accessor! {
        /// Extract the symbol's string index.
        as_symbol -> ArenaIndex, Value::Symbol(idx) => idx
    }

    value_accessor! {
        /// Extract the boolean value, or `Err(TypeError)` if not a boolean.
        as_bool -> bool, Value::Boolean(b) => b
    }

    value_accessor! {
        /// Extract the inner combiner of an applicative, or `Err(TypeError)`.
        as_applicative -> ArenaIndex, Value::Applicative(inner) => inner
    }

    value_accessor! {
        /// Extract the f64 value, or `Err(TypeError)` if not an f64.
        as_f64 -> f64, Value::F64(n) => n
    }

    value_accessor! {
        /// Extract the f32 value, or `Err(TypeError)` if not an f32.
        as_f32 -> f32, Value::F32(n) => n
    }

    /// Convert any numeric Value to isize, or `Err(TypeError)` if not numeric.
    pub fn to_isize(self) -> Result<isize, ArenaError> {
        match self {
            Value::Number(n) => Ok(n),
            Value::I8(n) => Ok(n as isize),
            Value::I16(n) => Ok(n as isize),
            Value::I32(n) => Ok(n as isize),
            Value::I64(n) => isize::try_from(n).map_err(|_| ArenaError::ArithmeticOverflow),
            Value::I128(n) => isize::try_from(n).map_err(|_| ArenaError::ArithmeticOverflow),
            Value::U8(n) => Ok(n as isize),
            Value::U16(n) => Ok(n as isize),
            Value::U32(n) => Ok(n as isize),
            Value::U64(n) => isize::try_from(n).map_err(|_| ArenaError::ArithmeticOverflow),
            Value::U128(n) => isize::try_from(n).map_err(|_| ArenaError::ArithmeticOverflow),
            Value::Usize(n) => isize::try_from(n).map_err(|_| ArenaError::ArithmeticOverflow),
            Value::F32(n) => Ok(n as isize),
            Value::F64(n) => Ok(n as isize),
            _ => Err(ArenaError::TypeError),
        }
    }
}

impl core::fmt::Display for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Value::Nil => f.write_str("()"),
            Value::Unit => f.write_str("#unit"),
            Value::Boolean(true) => f.write_str("#t"),
            Value::Boolean(false) => f.write_str("#f"),
            Value::Number(n) => write!(f, "{n}"),
            Value::Char(c) => write!(f, "#\\{c}"),
            Value::Inert => f.write_str("#inert"),
            Value::Ignore => f.write_str("#ignore"),
            Value::I8(n) => write!(f, "{n}i8"),
            Value::I16(n) => write!(f, "{n}i16"),
            Value::I32(n) => write!(f, "{n}i32"),
            Value::I64(n) => write!(f, "{n}i64"),
            Value::I128(n) => write!(f, "{n}i128"),
            Value::U8(n) => write!(f, "{n}u8"),
            Value::U16(n) => write!(f, "{n}u16"),
            Value::U32(n) => write!(f, "{n}u32"),
            Value::U64(n) => write!(f, "{n}u64"),
            Value::U128(n) => write!(f, "{n}u128"),
            Value::Usize(n) => write!(f, "{n}usize"),
            Value::F32(n) => write!(f, "{n}f32"),
            Value::F64(n) => write!(f, "{n}f64"),
            _ => write!(f, "<{}>", self.type_name()),
        }
    }
}

macro_rules! impl_from_value {
    ($($ty:ty => $variant:ident),+ $(,)?) => {
        $(impl From<$ty> for Value {
            #[inline]
            fn from(v: $ty) -> Self { Value::$variant(v) }
        })+
    };
}

impl_from_value!(
    bool => Boolean,
    isize => Number,
    char => Char,
    BuiltinId => Builtin,
    i8 => I8,
    i16 => I16,
    i32 => I32,
    i64 => I64,
    i128 => I128,
    u8 => U8,
    u16 => U16,
    u32 => U32,
    u64 => U64,
    u128 => U128,
    f32 => F32,
    f64 => F64,
);
