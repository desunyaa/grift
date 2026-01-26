#![no_std]

//! # Lisp Parser
//!
//! A classic Lisp parser with arena-allocated values.
//!
//! ## Design
//!
//! - Symbols are linked lists of `Char` values (classic Lisp style)
//! - All values are stored in a `pwn_arena` arena
//! - Supports garbage collection via the `Trace` trait
//!
//! ## Value Representation
//!
//! - `Nil` - The empty list / false
//! - `Number(i64)` - Integer numbers
//! - `Char(char)` - Single character
//! - `Cons { car, cdr }` - Pair/list cell
//! - `Symbol { chars }` - Symbol (tagged char list)
//! - `Lambda { params, body, env }` - Closure
//! - `Builtin(Builtin)` - Optimized built-in function

pub use pwn_arena::{Arena, ArenaIndex, ArenaError, ArenaResult, Trace, GcStats};

/// Built-in functions (optimization to avoid symbol lookup)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    // List operations
    Car,
    Cdr,
    Cons,
    List,
    
    // Predicates
    Atom,
    Eq,
    Null,
    Numberp,
    
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    
    // Comparison
    Lt,
    Gt,
    Le,
    Ge,
    NumEq,
    
    // I/O (for REPL)
    Print,
    Newline,
}

impl Builtin {
    /// Get the symbol name for this builtin
    pub const fn name(&self) -> &'static str {
        match self {
            Builtin::Car => "car",
            Builtin::Cdr => "cdr",
            Builtin::Cons => "cons",
            Builtin::List => "list",
            Builtin::Atom => "atom",
            Builtin::Eq => "eq",
            Builtin::Null => "null",
            Builtin::Numberp => "numberp",
            Builtin::Add => "+",
            Builtin::Sub => "-",
            Builtin::Mul => "*",
            Builtin::Div => "/",
            Builtin::Mod => "mod",
            Builtin::Lt => "<",
            Builtin::Gt => ">",
            Builtin::Le => "<=",
            Builtin::Ge => ">=",
            Builtin::NumEq => "=",
            Builtin::Print => "print",
            Builtin::Newline => "newline",
        }
    }
    
    /// All builtins for initialization
    pub const ALL: &'static [Builtin] = &[
        Builtin::Car, Builtin::Cdr, Builtin::Cons, Builtin::List,
        Builtin::Atom, Builtin::Eq, Builtin::Null, Builtin::Numberp,
        Builtin::Add, Builtin::Sub, Builtin::Mul, Builtin::Div, Builtin::Mod,
        Builtin::Lt, Builtin::Gt, Builtin::Le, Builtin::Ge, Builtin::NumEq,
        Builtin::Print, Builtin::Newline,
    ];
}

/// A Lisp value
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    /// The empty list / false value
    Nil,
    
    /// Integer number
    Number(i64),
    
    /// Single character (for building symbol char-lists)
    Char(char),
    
    /// Cons cell (pair)
    Cons {
        car: ArenaIndex,
        cdr: ArenaIndex,
    },
    
    /// Symbol (contains a linked list of Char values)
    Symbol {
        chars: ArenaIndex,
    },
    
    /// Lambda / closure
    Lambda {
        params: ArenaIndex,  // List of symbols
        body: ArenaIndex,    // Expression
        env: ArenaIndex,     // Captured environment (alist)
    },
    
    /// Built-in function (optimized)
    Builtin(Builtin),
}

impl Value {
    /// Check if this value is nil
    #[inline]
    pub const fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }
    
    /// Check if this value is an atom (not a cons cell)
    #[inline]
    pub const fn is_atom(&self) -> bool {
        !matches!(self, Value::Cons { .. })
    }
    
    /// Check if this value is a number
    #[inline]
    pub const fn is_number(&self) -> bool {
        matches!(self, Value::Number(_))
    }
    
    /// Check if this value is a symbol
    #[inline]
    pub const fn is_symbol(&self) -> bool {
        matches!(self, Value::Symbol { .. })
    }
    
    /// Check if this value is a cons cell
    #[inline]
    pub const fn is_cons(&self) -> bool {
        matches!(self, Value::Cons { .. })
    }
    
    /// Check if this value is a lambda
    #[inline]
    pub const fn is_lambda(&self) -> bool {
        matches!(self, Value::Lambda { .. })
    }
    
    /// Check if this value is a builtin
    #[inline]
    pub const fn is_builtin(&self) -> bool {
        matches!(self, Value::Builtin(_))
    }
    
    /// Get the number value if this is a number
    #[inline]
    pub const fn as_number(&self) -> Option<i64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }
    
    /// Get the char value if this is a char
    #[inline]
    pub const fn as_char(&self) -> Option<char> {
        match self {
            Value::Char(c) => Some(*c),
            _ => None,
        }
    }
}

/// Implement Trace for GC support
impl<const N: usize> Trace<Value, N> for Value {
    fn trace<F: FnMut(ArenaIndex)>(&self, mut tracer: F) {
        match self {
            Value::Nil | Value::Number(_) | Value::Char(_) | Value::Builtin(_) => {
                // No references
            }
            Value::Cons { car, cdr } => {
                tracer(*car);
                tracer(*cdr);
            }
            Value::Symbol { chars } => {
                tracer(*chars);
            }
            Value::Lambda { params, body, env } => {
                tracer(*params);
                tracer(*body);
                tracer(*env);
            }
        }
    }
}

// ============================================================================
// Lisp Context - Arena wrapper with helper methods
// ============================================================================

/// A Lisp execution context wrapping an arena
pub struct Lisp<const N: usize> {
    arena: Arena<Value, N>,
}

impl<const N: usize> Lisp<N> {
    /// Create a new Lisp context
    pub fn new() -> Self {
        Lisp {
            arena: Arena::new(Value::Nil),
        }
    }
    
    /// Get reference to the underlying arena
    pub fn arena(&self) -> &Arena<Value, N> {
        &self.arena
    }
    
    /// Allocate a value
    #[inline]
    pub fn alloc(&self, value: Value) -> ArenaResult<ArenaIndex> {
        self.arena.alloc(value)
    }
    
    /// Get a value
    #[inline]
    pub fn get(&self, index: ArenaIndex) -> ArenaResult<Value> {
        self.arena.get(index)
    }
    
    /// Set a value
    #[inline]
    pub fn set(&self, index: ArenaIndex, value: Value) -> ArenaResult<()> {
        self.arena.set(index, value)
    }
    
    /// Allocate Nil
    #[inline]
    pub fn nil(&self) -> ArenaResult<ArenaIndex> {
        self.alloc(Value::Nil)
    }
    
    /// Allocate a number
    #[inline]
    pub fn number(&self, n: i64) -> ArenaResult<ArenaIndex> {
        self.alloc(Value::Number(n))
    }
    
    /// Allocate a character
    #[inline]
    pub fn char(&self, c: char) -> ArenaResult<ArenaIndex> {
        self.alloc(Value::Char(c))
    }
    
    /// Allocate a cons cell
    #[inline]
    pub fn cons(&self, car: ArenaIndex, cdr: ArenaIndex) -> ArenaResult<ArenaIndex> {
        self.alloc(Value::Cons { car, cdr })
    }
    
    /// Get car of a cons cell
    pub fn car(&self, index: ArenaIndex) -> ArenaResult<ArenaIndex> {
        match self.get(index)? {
            Value::Cons { car, .. } => Ok(car),
            Value::Nil => self.nil(), // car of nil is nil in classic Lisp
            _ => Err(ArenaError::InvalidIndex),
        }
    }
    
    /// Get cdr of a cons cell
    pub fn cdr(&self, index: ArenaIndex) -> ArenaResult<ArenaIndex> {
        match self.get(index)? {
            Value::Cons { cdr, .. } => Ok(cdr),
            Value::Nil => self.nil(), // cdr of nil is nil in classic Lisp
            _ => Err(ArenaError::InvalidIndex),
        }
    }
    
    /// Create a symbol from a string slice (builds char list)
    pub fn symbol(&self, name: &str) -> ArenaResult<ArenaIndex> {
        // Build linked list of chars in reverse, then it's already correct
        // because we build from the end
        let mut chars = self.nil()?;
        
        // Build in reverse order
        for c in name.chars().rev() {
            let char_val = self.char(c)?;
            chars = self.cons(char_val, chars)?;
        }
        
        self.alloc(Value::Symbol { chars })
    }
    
    /// Create a symbol from bytes (for parsing)
    pub fn symbol_from_bytes(&self, bytes: &[u8]) -> ArenaResult<ArenaIndex> {
        let mut chars = self.nil()?;
        
        // Build in reverse order
        for &b in bytes.iter().rev() {
            let char_val = self.char(b as char)?;
            chars = self.cons(char_val, chars)?;
        }
        
        self.alloc(Value::Symbol { chars })
    }
    
    /// Allocate a builtin function
    #[inline]
    pub fn builtin(&self, b: Builtin) -> ArenaResult<ArenaIndex> {
        self.alloc(Value::Builtin(b))
    }
    
    /// Allocate a lambda
    pub fn lambda(&self, params: ArenaIndex, body: ArenaIndex, env: ArenaIndex) -> ArenaResult<ArenaIndex> {
        self.alloc(Value::Lambda { params, body, env })
    }
    
    /// Build a list from an iterator of indices
    pub fn list<I: IntoIterator<Item = ArenaIndex>>(&self, items: I) -> ArenaResult<ArenaIndex>
    where
        I::IntoIter: DoubleEndedIterator,
    {
        let mut result = self.nil()?;
        for item in items.into_iter().rev() {
            result = self.cons(item, result)?;
        }
        Ok(result)
    }
    
    /// Get the length of a list
    pub fn list_len(&self, mut list: ArenaIndex) -> ArenaResult<usize> {
        let mut len = 0;
        loop {
            match self.get(list)? {
                Value::Nil => return Ok(len),
                Value::Cons { cdr, .. } => {
                    len += 1;
                    list = cdr;
                }
                _ => return Err(ArenaError::InvalidIndex), // Not a proper list
            }
        }
    }
    
    /// Check if two symbols are equal (compare char lists)
    pub fn symbol_eq(&self, a: ArenaIndex, b: ArenaIndex) -> ArenaResult<bool> {
        let val_a = self.get(a)?;
        let val_b = self.get(b)?;
        
        match (val_a, val_b) {
            (Value::Symbol { chars: chars_a }, Value::Symbol { chars: chars_b }) => {
                self.char_list_eq(chars_a, chars_b)
            }
            _ => Ok(false),
        }
    }
    
    /// Compare two char lists for equality
    fn char_list_eq(&self, mut a: ArenaIndex, mut b: ArenaIndex) -> ArenaResult<bool> {
        loop {
            let val_a = self.get(a)?;
            let val_b = self.get(b)?;
            
            match (val_a, val_b) {
                (Value::Nil, Value::Nil) => return Ok(true),
                (Value::Cons { car: car_a, cdr: cdr_a }, Value::Cons { car: car_b, cdr: cdr_b }) => {
                    let char_a = self.get(car_a)?;
                    let char_b = self.get(car_b)?;
                    
                    match (char_a, char_b) {
                        (Value::Char(c1), Value::Char(c2)) if c1 == c2 => {
                            a = cdr_a;
                            b = cdr_b;
                        }
                        _ => return Ok(false),
                    }
                }
                _ => return Ok(false),
            }
        }
    }
    
    /// Check if a symbol matches a string
    pub fn symbol_matches(&self, sym: ArenaIndex, name: &str) -> ArenaResult<bool> {
        let val = self.get(sym)?;
        
        match val {
            Value::Symbol { chars } => {
                let mut list = chars;
                let mut chars_iter = name.chars();
                
                loop {
                    let list_val = self.get(list)?;
                    let next_char = chars_iter.next();
                    
                    match (list_val, next_char) {
                        (Value::Nil, None) => return Ok(true),
                        (Value::Cons { car, cdr }, Some(expected)) => {
                            if let Value::Char(c) = self.get(car)? {
                                if c != expected {
                                    return Ok(false);
                                }
                                list = cdr;
                            } else {
                                return Ok(false);
                            }
                        }
                        _ => return Ok(false),
                    }
                }
            }
            _ => Ok(false),
        }
    }
    
    /// Run garbage collection
    pub fn gc(&self, roots: &[ArenaIndex]) -> GcStats {
        self.arena.collect_garbage(roots)
    }
    
    /// Allocate with GC on failure
    pub fn alloc_or_gc(&self, value: Value, roots: &[ArenaIndex]) -> ArenaResult<ArenaIndex> {
        self.arena.alloc_or_gc(value, roots)
    }
    
    /// Get arena stats
    pub fn stats(&self) -> pwn_arena::ArenaStats {
        self.arena.stats()
    }
}

impl<const N: usize> Default for Lisp<N> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Parser
// ============================================================================

/// Parser error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// Unexpected end of input
    UnexpectedEof,
    /// Unexpected character
    UnexpectedChar(char),
    /// Unmatched parenthesis
    UnmatchedParen,
    /// Number too large
    NumberOverflow,
    /// Arena is full
    OutOfMemory,
}

impl From<ArenaError> for ParseError {
    fn from(e: ArenaError) -> Self {
        match e {
            ArenaError::OutOfMemory => ParseError::OutOfMemory,
            _ => ParseError::OutOfMemory, // Shouldn't happen during parsing
        }
    }
}

/// Parser state
pub struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    /// Create a new parser
    pub fn new(input: &'a str) -> Self {
        Parser {
            input: input.as_bytes(),
            pos: 0,
        }
    }
    
    /// Create from bytes
    pub fn from_bytes(input: &'a [u8]) -> Self {
        Parser { input, pos: 0 }
    }
    
    /// Peek at the current character
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }
    
    /// Advance and return the current character
    fn advance(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }
    
    /// Skip whitespace and comments
    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                self.advance();
            } else if c == b';' {
                // Comment - skip to end of line
                while let Some(c) = self.advance() {
                    if c == b'\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }
    
    /// Check if a character can be part of a symbol
    fn is_symbol_char(c: u8) -> bool {
        matches!(c, 
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' |
            b'+' | b'-' | b'*' | b'/' | b'<' | b'>' | b'=' |
            b'?' | b'!' | b'_' | b'&' | b'%' | b'^' | b'~'
        )
    }
    
    /// Parse a single expression
    pub fn parse<const N: usize>(&mut self, lisp: &Lisp<N>) -> Result<ArenaIndex, ParseError> {
        self.skip_whitespace();
        
        match self.peek() {
            None => Err(ParseError::UnexpectedEof),
            
            Some(b'(') => self.parse_list(lisp),
            
            Some(b')') => Err(ParseError::UnmatchedParen),
            
            Some(b'\'') => {
                // Quote: 'x -> (quote x)
                self.advance();
                let expr = self.parse(lisp)?;
                let quote_sym = lisp.symbol("quote")?;
                let nil = lisp.nil()?;
                let quoted = lisp.cons(expr, nil)?;
                lisp.cons(quote_sym, quoted).map_err(Into::into)
            }
            
            Some(c) if c.is_ascii_digit() => self.parse_number(lisp),
            
            Some(b'-') => {
                // Could be negative number or symbol
                if self.input.get(self.pos + 1).map_or(false, |c| c.is_ascii_digit()) {
                    self.parse_number(lisp)
                } else {
                    self.parse_symbol(lisp)
                }
            }
            
            Some(c) if Self::is_symbol_char(c) => self.parse_symbol(lisp),
            
            Some(c) => Err(ParseError::UnexpectedChar(c as char)),
        }
    }
    
    /// Parse a list (including nil)
    fn parse_list<const N: usize>(&mut self, lisp: &Lisp<N>) -> Result<ArenaIndex, ParseError> {
        self.advance(); // consume '('
        self.skip_whitespace();
        
        if self.peek() == Some(b')') {
            self.advance();
            return lisp.nil().map_err(Into::into);
        }
        
        // Parse elements and build list
        // We need to build in order, so we use a temporary stack
        // Since we're no_std, we use a fixed-size buffer
        const MAX_LIST_DEPTH: usize = 64;
        let mut elements: [ArenaIndex; MAX_LIST_DEPTH] = [ArenaIndex::NULL; MAX_LIST_DEPTH];
        let mut count = 0;
        
        loop {
            self.skip_whitespace();
            
            match self.peek() {
                None => return Err(ParseError::UnexpectedEof),
                Some(b')') => {
                    self.advance();
                    break;
                }
                Some(b'.') => {
                    // Dotted pair: (a . b)
                    self.advance();
                    self.skip_whitespace();
                    
                    if count == 0 {
                        return Err(ParseError::UnexpectedChar('.'));
                    }
                    
                    let cdr = self.parse(lisp)?;
                    self.skip_whitespace();
                    
                    if self.advance() != Some(b')') {
                        return Err(ParseError::UnmatchedParen);
                    }
                    
                    // Build the dotted list
                    let mut result = cdr;
                    for i in (0..count).rev() {
                        result = lisp.cons(elements[i], result)?;
                    }
                    return Ok(result);
                }
                Some(_) => {
                    if count >= MAX_LIST_DEPTH {
                        return Err(ParseError::OutOfMemory);
                    }
                    elements[count] = self.parse(lisp)?;
                    count += 1;
                }
            }
        }
        
        // Build proper list
        let mut result = lisp.nil()?;
        for i in (0..count).rev() {
            result = lisp.cons(elements[i], result)?;
        }
        Ok(result)
    }
    
    /// Parse a number
    fn parse_number<const N: usize>(&mut self, lisp: &Lisp<N>) -> Result<ArenaIndex, ParseError> {
        let mut value: i64 = 0;
        let negative = if self.peek() == Some(b'-') {
            self.advance();
            true
        } else {
            false
        };
        
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.advance();
                value = value.checked_mul(10)
                    .and_then(|v| v.checked_add((c - b'0') as i64))
                    .ok_or(ParseError::NumberOverflow)?;
            } else {
                break;
            }
        }
        
        if negative {
            value = -value;
        }
        
        lisp.number(value).map_err(Into::into)
    }
    
    /// Parse a symbol
    fn parse_symbol<const N: usize>(&mut self, lisp: &Lisp<N>) -> Result<ArenaIndex, ParseError> {
        const MAX_SYMBOL_LEN: usize = 64;
        let mut buffer: [u8; MAX_SYMBOL_LEN] = [0; MAX_SYMBOL_LEN];
        let mut len = 0;
        
        while let Some(c) = self.peek() {
            if Self::is_symbol_char(c) && len < MAX_SYMBOL_LEN {
                buffer[len] = c.to_ascii_lowercase();
                len += 1;
                self.advance();
            } else {
                break;
            }
        }
        
        let name = &buffer[..len];
        
        // Check for special symbols
        if name == b"nil" {
            return lisp.nil().map_err(Into::into);
        }
        if name == b"t" {
            return lisp.symbol("t").map_err(Into::into);
        }
        
        lisp.symbol_from_bytes(name).map_err(Into::into)
    }
    
    /// Check if there's more input (after whitespace)
    pub fn has_more(&mut self) -> bool {
        self.skip_whitespace();
        self.peek().is_some()
    }
}

/// Parse a string into a Lisp expression
pub fn parse<const N: usize>(lisp: &Lisp<N>, input: &str) -> Result<ArenaIndex, ParseError> {
    let mut parser = Parser::new(input);
    parser.parse(lisp)
}

/// Parse multiple expressions
pub fn parse_all<const N: usize>(lisp: &Lisp<N>, input: &str) -> Result<ArenaIndex, ParseError> {
    let mut parser = Parser::new(input);
    let mut results = [ArenaIndex::NULL; 64];
    let mut count = 0;
    
    while parser.has_more() && count < 64 {
        results[count] = parser.parse(lisp)?;
        count += 1;
    }
    
    // Build list of results
    let mut result = lisp.nil()?;
    for i in (0..count).rev() {
        result = lisp.cons(results[i], result)?;
    }
    Ok(result)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_number() {
        let lisp: Lisp<100> = Lisp::new();
        
        let idx = parse(&lisp, "42").unwrap();
        assert_eq!(lisp.get(idx).unwrap(), Value::Number(42));
        
        let idx = parse(&lisp, "-123").unwrap();
        assert_eq!(lisp.get(idx).unwrap(), Value::Number(-123));
    }
    
    #[test]
    fn test_parse_symbol() {
        let lisp: Lisp<100> = Lisp::new();
        
        let idx = parse(&lisp, "hello").unwrap();
        assert!(lisp.symbol_matches(idx, "hello").unwrap());
    }
    
    #[test]
    fn test_parse_nil() {
        let lisp: Lisp<100> = Lisp::new();
        
        let idx = parse(&lisp, "nil").unwrap();
        assert_eq!(lisp.get(idx).unwrap(), Value::Nil);
        
        let idx = parse(&lisp, "()").unwrap();
        assert_eq!(lisp.get(idx).unwrap(), Value::Nil);
    }
    
    #[test]
    fn test_parse_list() {
        let lisp: Lisp<100> = Lisp::new();
        
        let idx = parse(&lisp, "(1 2 3)").unwrap();
        
        // Check it's a cons
        let val = lisp.get(idx).unwrap();
        assert!(val.is_cons());
        
        // Check first element
        let car = lisp.car(idx).unwrap();
        assert_eq!(lisp.get(car).unwrap(), Value::Number(1));
    }
    
    #[test]
    fn test_parse_nested() {
        let lisp: Lisp<100> = Lisp::new();
        
        let idx = parse(&lisp, "(+ 1 (- 3 2))").unwrap();
        assert!(lisp.get(idx).unwrap().is_cons());
    }
    
    #[test]
    fn test_parse_quote() {
        let lisp: Lisp<100> = Lisp::new();
        
        let idx = parse(&lisp, "'x").unwrap();
        
        // Should be (quote x)
        let car = lisp.car(idx).unwrap();
        assert!(lisp.symbol_matches(car, "quote").unwrap());
    }
    
    #[test]
    fn test_symbol_equality() {
        let lisp: Lisp<100> = Lisp::new();
        
        let a = lisp.symbol("hello").unwrap();
        let b = lisp.symbol("hello").unwrap();
        let c = lisp.symbol("world").unwrap();
        
        assert!(lisp.symbol_eq(a, b).unwrap());
        assert!(!lisp.symbol_eq(a, c).unwrap());
    }
    
    #[test]
    fn test_gc() {
        let lisp: Lisp<100> = Lisp::new();
        
        let root = parse(&lisp, "(1 2 3)").unwrap();
        
        // Allocate garbage
        for i in 0..20 {
            lisp.number(i * 1000).unwrap();
        }
        
        let stats = lisp.gc(&[root]);
        assert!(stats.collected > 0);
    }
}
