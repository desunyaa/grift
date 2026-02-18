//! S-expression parser.
//!
//! Tokenizes and parses Lisp source text into arena-allocated values.

use grift_arena::{ArenaIndex, ArenaError, ArenaResult, Arena};

use crate::lisp::Lisp;
use crate::value::Value;

/// A simple S-expression parser.
pub(crate) struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    /// Create a new parser for the given input string.
    pub fn new(input: &'a str) -> Self {
        Parser {
            input: input.as_bytes(),
            pos: 0,
        }
    }

    /// Parse one expression.
    pub fn parse<const N: usize>(&mut self, lisp: &Lisp<N>) -> ArenaResult<ArenaIndex> {
        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return lisp.nil();
        }

        match self.input[self.pos] {
            b'(' => {
                self.pos += 1;
                self.parse_list(lisp)
            }
            b'\'' => {
                self.pos += 1;
                self.parse_quote(lisp)
            }
            b'"' => {
                self.pos += 1;
                self.parse_string_literal(lisp)
            }
            b')' => Err(ArenaError::ParseError),
            _ => self.parse_atom(lisp),
        }
    }

    /// Skip whitespace and comments.
    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            match self.input[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                b';' => {
                    while self.pos < self.input.len() && self.input[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    /// Parse a list: `(a b c)` → cons cells.
    fn parse_list<const N: usize>(&mut self, lisp: &Lisp<N>) -> ArenaResult<ArenaIndex> {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Err(ArenaError::ParseError);
        }

        if self.input[self.pos] == b')' {
            self.pos += 1;
            return lisp.nil();
        }

        if self.peek_dot() {
            return Err(ArenaError::ParseError);
        }

        let car = self.parse(lisp)?;

        self.skip_whitespace();
        if self.peek_dot() {
            self.pos += 1; // skip the dot
            let cdr = self.parse(lisp)?;
            self.skip_whitespace();
            if self.pos < self.input.len() && self.input[self.pos] == b')' {
                self.pos += 1;
                return lisp.cons(car, cdr);
            }
            return Err(ArenaError::ParseError);
        }

        let cdr = self.parse_list(lisp)?;
        lisp.cons(car, cdr)
    }

    /// Check if current position is a dot separator (not a number like `.5`).
    fn peek_dot(&self) -> bool {
        self.input.get(self.pos) == Some(&b'.')
            && self.input.get(self.pos + 1)
                .is_none_or(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b')'))
    }

    /// Parse `'expr` → `(quote expr)`.
    fn parse_quote<const N: usize>(&mut self, lisp: &Lisp<N>) -> ArenaResult<ArenaIndex> {
        let expr = self.parse(lisp)?;
        let quote_sym = lisp.symbol("quote")?;
        let inner = lisp.cons(expr, ArenaIndex::NIL)?;
        lisp.cons(quote_sym, inner)
    }

    /// Parse a string literal `"..."`.
    fn parse_string_literal<const N: usize>(
        &mut self,
        lisp: &Lisp<N>,
    ) -> ArenaResult<ArenaIndex> {
        let start = self.pos;
        while self.pos < self.input.len() && self.input[self.pos] != b'"' {
            if self.input[self.pos] == b'\\' {
                self.pos += 1; // skip escaped char
            }
            self.pos += 1;
        }
        if self.pos >= self.input.len() {
            return Err(ArenaError::ParseError);
        }
        let end = self.pos;
        self.pos += 1; // skip closing quote

        let slice = &self.input[start..end];
        let s = core::str::from_utf8(slice).map_err(|_| ArenaError::ParseError)?;
        lisp.alloc_string(s)
    }

    /// Parse an atom: number, symbol, or boolean.
    fn parse_atom<const N: usize>(&mut self, lisp: &Lisp<N>) -> ArenaResult<ArenaIndex> {
        let start = self.pos;
        while self.pos < self.input.len() {
            match self.input[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' | b'(' | b')' | b'"' | b';' => break,
                _ => self.pos += 1,
            }
        }

        let token = &self.input[start..self.pos];
        let s = core::str::from_utf8(token).map_err(|_| ArenaError::ParseError)?;

        match s {
            "#t" | "#true" => lisp.boolean(true),
            "#f" | "#false" => lisp.boolean(false),
            "#inert" => lisp.inert(),
            "#ignore" => lisp.ignore(),
            "#unit" => lisp.unit(),
            _ => parse_typed_number(s, lisp)
                .unwrap_or_else(|| lisp.symbol(s)),
        }
    }
}

/// Parse an integer from a string slice without using std.
fn parse_integer(s: &str) -> Option<isize> {
    let bytes = s.as_bytes();
    let (&first, rest) = bytes.split_first()?;

    let (negative, digits) = match first {
        b'-' if !rest.is_empty() => (true, rest),
        b'+' if !rest.is_empty() => (false, rest),
        b'0'..=b'9' => (false, bytes),
        _ => return None,
    };

    let magnitude = digits.iter().try_fold(0isize, |acc, &b| {
        if !b.is_ascii_digit() { return None; }
        acc.checked_mul(10)?.checked_add((b - b'0') as isize)
    })?;

    Some(if negative { -magnitude } else { magnitude })
}

/// Try to parse a typed number literal (with optional suffix) and allocate it.
fn parse_typed_number<const N: usize>(s: &str, lisp: &Lisp<N>) -> Option<ArenaResult<ArenaIndex>> {
    // Check for type suffixes first.
    if let Some(result) = parse_suffixed_number(s, lisp) {
        return Some(result);
    }
    // Try plain integer.
    if let Some(n) = parse_integer(s) {
        return Some(lisp.number(n));
    }
    // Try float (contains '.' or 'e'/'E' but no type suffix).
    if s.as_bytes().iter().any(|&b| b == b'.' || b == b'e' || b == b'E') {
        if let Some(f) = parse_f64(s) {
            return Some(lisp.arena.alloc(Value::F64(f)));
        }
    }
    None
}

/// Parse a number with a type suffix (e.g., `42i8`, `3.14f32`).
fn parse_suffixed_number<const N: usize>(s: &str, lisp: &Lisp<N>) -> Option<ArenaResult<ArenaIndex>> {
    // Try each suffix from longest to shortest to avoid ambiguity.
    static SUFFIXES: &[(&str, fn(&str, &Arena<Value, 1>) -> Option<Value>)] = &[];
    let _ = SUFFIXES; // suppress unused warning

    macro_rules! try_suffix {
        ($s:expr, $suffix:literal, $ty:ty, $variant:ident, $lisp:expr) => {
            if let Some(prefix) = $s.strip_suffix($suffix) {
                if let Some(n) = parse_integer(prefix) {
                    let val = <$ty>::try_from(n).ok()?;
                    return Some($lisp.arena.alloc(Value::$variant(val)));
                }
            }
        };
        // Float suffix variant
        (float $s:expr, $suffix:literal, $ty:ty, $variant:ident, $lisp:expr) => {
            if let Some(prefix) = $s.strip_suffix($suffix) {
                if let Some(f) = parse_f64(prefix) {
                    return Some($lisp.arena.alloc(Value::$variant(f as $ty)));
                }
                if let Some(n) = parse_integer(prefix) {
                    return Some($lisp.arena.alloc(Value::$variant(n as $ty)));
                }
            }
        };
    }

    // Integer suffixes (longest first to avoid ambiguity: i128 before i16 before i8).
    try_suffix!(s, "i128", i128, I128, lisp);
    try_suffix!(s, "i16", i16, I16, lisp);
    try_suffix!(s, "i32", i32, I32, lisp);
    try_suffix!(s, "i64", i64, I64, lisp);
    try_suffix!(s, "isize", isize, Number, lisp);
    try_suffix!(s, "i8", i8, I8, lisp);

    try_suffix!(s, "u128", u128, U128, lisp);
    try_suffix!(s, "u16", u16, U16, lisp);
    try_suffix!(s, "u32", u32, U32, lisp);
    try_suffix!(s, "u64", u64, U64, lisp);
    try_suffix!(s, "usize", usize, Usize, lisp);
    try_suffix!(s, "u8", u8, U8, lisp);

    // Float suffixes.
    try_suffix!(float s, "f32", f32, F32, lisp);
    try_suffix!(float s, "f64", f64, F64, lisp);

    None
}

/// Parse a floating-point number from a string slice without std.
fn parse_f64(s: &str) -> Option<f64> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let (&first, rest) = bytes.split_first()?;
    let (negative, start) = match first {
        b'-' if !rest.is_empty() => (true, 1),
        b'+' if !rest.is_empty() => (false, 1),
        b'0'..=b'9' | b'.' => (false, 0),
        _ => return None,
    };

    let mut integer_part: f64 = 0.0;
    let mut i = start;

    // Parse integer part.
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        integer_part = integer_part * 10.0 + (bytes[i] - b'0') as f64;
        i += 1;
    }

    // Parse fractional part.
    let mut frac_part: f64 = 0.0;
    let mut frac_scale: f64 = 1.0;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            frac_part = frac_part * 10.0 + (bytes[i] - b'0') as f64;
            frac_scale *= 10.0;
            i += 1;
        }
    }

    // Parse exponent.
    let mut exponent: i32 = 0;
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        let exp_negative = if i < bytes.len() && bytes[i] == b'-' {
            i += 1;
            true
        } else if i < bytes.len() && bytes[i] == b'+' {
            i += 1;
            false
        } else {
            false
        };
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            exponent = exponent.checked_mul(10)?.checked_add((bytes[i] - b'0') as i32)?;
            i += 1;
        }
        if exp_negative {
            exponent = -exponent;
        }
    }

    // Must have consumed entire string.
    if i != bytes.len() {
        return None;
    }

    let mut result = integer_part + frac_part / frac_scale;

    // Apply exponent via repeated multiply/divide.
    if exponent > 0 {
        let mut e = exponent;
        while e > 0 {
            result *= 10.0;
            e -= 1;
        }
    } else if exponent < 0 {
        let mut e = -exponent;
        while e > 0 {
            result /= 10.0;
            e -= 1;
        }
    }

    Some(if negative { -result } else { result })
}
