#![no_std]

//! # Lisp Evaluator
//!
//! A classic Lisp evaluator with:
//! - Lexically scoped closures
//! - Tail Call Optimization (TCO)
//! - Call-by-need (lazy evaluation via delay/force)
//! - Full mutation (set-car!, set-cdr!)
//! - Rich error handling with stack traces
//!
//! ## Truthiness
//!
//! Only `#f` is false. Everything else (including `nil`/`'()`) is truthy.
//!
//! ## Special Forms
//!
//! - `quote` - Return expression unevaluated
//! - `if` - Conditional (TCO in branches)
//! - `cond` - Multi-way conditional (TCO in last branch)
//! - `lambda` - Create closure
//! - `define` - Define variable/function
//! - `let` - Local binding
//! - `let*` - Sequential local binding
//! - `begin` - Sequence of expressions (TCO in last)
//! - `set!` - Mutation of bindings
//! - `and` / `or` - Short-circuit boolean operations
//! - `delay` - Create a thunk (lazy evaluation)

pub use lisp_parser::{
    Arena, ArenaIndex, ArenaError, ArenaResult, Trace, GcStats,
    Value, Builtin, Lisp, ParseError, ParseErrorKind, SourceLoc, parse,
};

// ============================================================================
// Error Handling
// ============================================================================

/// Maximum call stack depth for traces
const MAX_STACK_DEPTH: usize = 64;
/// Maximum frames to include in error backtrace
const MAX_BACKTRACE: usize = 16;

/// Error kind enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Arena is full
    OutOfMemory,
    /// Unbound variable
    UnboundVariable,
    /// Not a function
    NotAFunction,
    /// Wrong number of arguments
    WrongArgCount,
    /// Type error
    TypeError,
    /// Division by zero
    DivisionByZero,
    /// Parse error
    Parse,
    /// User-raised error
    UserError,
    /// Stack overflow (recursion too deep)
    StackOverflow,
    /// Cannot mutate non-pair
    NotAPair,
    /// Generic error
    Generic,
}

impl ErrorKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            ErrorKind::OutOfMemory => "out of memory",
            ErrorKind::UnboundVariable => "unbound variable",
            ErrorKind::NotAFunction => "not a function",
            ErrorKind::WrongArgCount => "wrong number of arguments",
            ErrorKind::TypeError => "type error",
            ErrorKind::DivisionByZero => "division by zero",
            ErrorKind::Parse => "parse error",
            ErrorKind::UserError => "error",
            ErrorKind::StackOverflow => "stack overflow",
            ErrorKind::NotAPair => "not a pair",
            ErrorKind::Generic => "error",
        }
    }
}

/// A stack frame for error reporting
#[derive(Clone, Copy, Debug)]
pub struct StackFrame {
    /// The expression being evaluated (for display)
    pub expr: ArenaIndex,
    /// Function being called (if applicable)
    pub func: ArenaIndex,
}

impl Default for StackFrame {
    fn default() -> Self {
        StackFrame {
            expr: ArenaIndex::NULL,
            func: ArenaIndex::NULL,
        }
    }
}

/// Evaluation error with context
#[derive(Debug)]
pub struct EvalError {
    /// What kind of error
    pub kind: ErrorKind,
    /// Human-readable message context
    pub message: ErrorMessage,
    /// The expression that caused the error
    pub expr: ArenaIndex,
    /// Expected type (for type errors)
    pub expected: Option<&'static str>,
    /// Got type (for type errors)
    pub got: Option<&'static str>,
    /// Expected argument count
    pub expected_args: Option<usize>,
    /// Got argument count
    pub got_args: Option<usize>,
    /// Call stack backtrace
    pub backtrace: [StackFrame; MAX_BACKTRACE],
    pub backtrace_len: usize,
    /// Original parse error (if applicable)
    pub parse_error: Option<ParseError>,
}

/// Fixed-size message buffer for no_std
#[derive(Debug, Clone, Copy)]
pub struct ErrorMessage {
    buf: [u8; 64],
    len: usize,
}

impl ErrorMessage {
    pub const fn empty() -> Self {
        ErrorMessage { buf: [0; 64], len: 0 }
    }
    
    pub fn from_str(s: &str) -> Self {
        let mut msg = ErrorMessage::empty();
        let bytes = s.as_bytes();
        let len = bytes.len().min(64);
        msg.buf[..len].copy_from_slice(&bytes[..len]);
        msg.len = len;
        msg
    }
    
    pub fn as_str(&self) -> &str {
        // Safety: we only store valid UTF-8
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.len]) }
    }
}

impl Default for ErrorMessage {
    fn default() -> Self {
        Self::empty()
    }
}

impl EvalError {
    pub fn new(kind: ErrorKind) -> Self {
        EvalError {
            kind,
            message: ErrorMessage::empty(),
            expr: ArenaIndex::NULL,
            expected: None,
            got: None,
            expected_args: None,
            got_args: None,
            backtrace: [StackFrame::default(); MAX_BACKTRACE],
            backtrace_len: 0,
            parse_error: None,
        }
    }
    
    pub fn with_expr(mut self, expr: ArenaIndex) -> Self {
        self.expr = expr;
        self
    }
    
    pub fn with_message(mut self, msg: &str) -> Self {
        self.message = ErrorMessage::from_str(msg);
        self
    }
    
    pub fn with_types(mut self, expected: &'static str, got: &'static str) -> Self {
        self.expected = Some(expected);
        self.got = Some(got);
        self
    }
    
    pub fn with_args(mut self, expected: usize, got: usize) -> Self {
        self.expected_args = Some(expected);
        self.got_args = Some(got);
        self
    }
    
    pub fn with_backtrace(mut self, stack: &[StackFrame], len: usize) -> Self {
        let copy_len = len.min(MAX_BACKTRACE);
        self.backtrace[..copy_len].copy_from_slice(&stack[..copy_len]);
        self.backtrace_len = copy_len;
        self
    }
}

impl From<ArenaError> for EvalError {
    fn from(e: ArenaError) -> Self {
        match e {
            ArenaError::OutOfMemory => EvalError::new(ErrorKind::OutOfMemory),
            _ => EvalError::new(ErrorKind::Generic),
        }
    }
}

impl From<ParseError> for EvalError {
    fn from(e: ParseError) -> Self {
        let mut err = EvalError::new(ErrorKind::Parse);
        err.parse_error = Some(e);
        err
    }
}

/// Result type for evaluation
pub type EvalResult = Result<ArenaIndex, EvalError>;

/// Result for TCO helper functions (internal)
enum TcoResult {
    /// Return this value immediately
    Return(ArenaIndex),
    /// Continue evaluation with new expression and environment (tail call)
    TailCall { new_expr: ArenaIndex, new_env: ArenaIndex },
}

// ============================================================================
// Evaluator
// ============================================================================

/// The Lisp evaluator with TCO support
pub struct Evaluator<'a, const N: usize> {
    lisp: &'a Lisp<N>,
    /// Global environment
    global_env: ArenaIndex,
    /// Call stack for error reporting
    call_stack: [StackFrame; MAX_STACK_DEPTH],
    call_stack_depth: usize,
}

impl<'a, const N: usize> Evaluator<'a, N> {
    /// Create a new evaluator with standard environment
    pub fn new(lisp: &'a Lisp<N>) -> Result<Self, EvalError> {
        let mut eval = Evaluator {
            lisp,
            global_env: ArenaIndex::NULL,
            call_stack: [StackFrame::default(); MAX_STACK_DEPTH],
            call_stack_depth: 0,
        };
        
        // Initialize global environment with builtins
        eval.global_env = lisp.nil()?;
        
        for &builtin in Builtin::ALL {
            let name = lisp.symbol(builtin.name())?;
            let val = lisp.builtin(builtin)?;
            eval.global_env = eval.env_extend(eval.global_env, name, val)?;
        }
        
        // Add 'true' and 'false' as aliases for #t and #f
        let true_sym = lisp.symbol("true")?;
        let true_val = lisp.true_val()?;
        eval.global_env = eval.env_extend(eval.global_env, true_sym, true_val)?;
        
        let false_sym = lisp.symbol("false")?;
        let false_val = lisp.false_val()?;
        eval.global_env = eval.env_extend(eval.global_env, false_sym, false_val)?;
        
        Ok(eval)
    }
    
    /// Get the Lisp context
    pub fn lisp(&self) -> &Lisp<N> {
        self.lisp
    }
    
    /// Get the global environment
    pub fn global_env(&self) -> ArenaIndex {
        self.global_env
    }
    
    /// Run GC with current roots
    pub fn gc(&self) -> GcStats {
        self.lisp.gc(&[self.global_env])
    }
    
    // ========================================================================
    // Stack Management
    // ========================================================================
    
    fn push_frame(&mut self, expr: ArenaIndex, func: ArenaIndex) -> Result<(), EvalError> {
        if self.call_stack_depth >= MAX_STACK_DEPTH {
            return Err(self.make_error(ErrorKind::StackOverflow, expr));
        }
        self.call_stack[self.call_stack_depth] = StackFrame { expr, func };
        self.call_stack_depth += 1;
        Ok(())
    }
    
    fn pop_frame(&mut self) {
        if self.call_stack_depth > 0 {
            self.call_stack_depth -= 1;
        }
    }
    
    fn make_error(&self, kind: ErrorKind, expr: ArenaIndex) -> EvalError {
        EvalError::new(kind)
            .with_expr(expr)
            .with_backtrace(&self.call_stack, self.call_stack_depth)
    }
    
    fn type_error(&self, expr: ArenaIndex, expected: &'static str, got: &'static str) -> EvalError {
        self.make_error(ErrorKind::TypeError, expr)
            .with_types(expected, got)
    }
    
    fn arg_error(&self, expr: ArenaIndex, expected: usize, got: usize) -> EvalError {
        self.make_error(ErrorKind::WrongArgCount, expr)
            .with_args(expected, got)
    }
    
    // ========================================================================
    // Environment Management
    // ========================================================================
    
    /// Extend an environment with a binding
    fn env_extend(&self, env: ArenaIndex, name: ArenaIndex, value: ArenaIndex) -> EvalResult {
        let binding = self.lisp.cons(name, value)?;
        self.lisp.cons(binding, env).map_err(Into::into)
    }
    
    /// Look up a variable in an environment
    fn env_lookup(&self, env: ArenaIndex, name: ArenaIndex) -> EvalResult {
        let mut current = env;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => {
                    // Try global
                    return self.env_lookup_global(name);
                }
                Value::Cons { car, cdr } => {
                    let binding = self.lisp.get(car)?;
                    if let Value::Cons { car: bound_name, cdr: bound_value } = binding {
                        if self.lisp.symbol_eq(bound_name, name)? {
                            return Ok(bound_value);
                        }
                    }
                    current = cdr;
                }
                _ => return Err(self.make_error(ErrorKind::Generic, name)),
            }
        }
    }
    
    /// Look up in global environment only
    fn env_lookup_global(&self, name: ArenaIndex) -> EvalResult {
        let mut current = self.global_env;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => {
                    return Err(self.make_error(ErrorKind::UnboundVariable, name));
                }
                Value::Cons { car, cdr } => {
                    let binding = self.lisp.get(car)?;
                    if let Value::Cons { car: bound_name, cdr: bound_value } = binding {
                        if self.lisp.symbol_eq(bound_name, name)? {
                            return Ok(bound_value);
                        }
                    }
                    current = cdr;
                }
                _ => return Err(self.make_error(ErrorKind::Generic, name)),
            }
        }
    }
    
    /// Set a variable in the environment (mutation)
    fn env_set(&self, env: ArenaIndex, name: ArenaIndex, value: ArenaIndex) -> EvalResult {
        // Try local first
        let mut current = env;
        loop {
            match self.lisp.get(current)? {
                Value::Nil => break,
                Value::Cons { car, cdr } => {
                    let binding = self.lisp.get(car)?;
                    if let Value::Cons { car: bound_name, cdr: _ } = binding {
                        if self.lisp.symbol_eq(bound_name, name)? {
                            self.lisp.set(car, Value::Cons { car: bound_name, cdr: value })?;
                            return Ok(value);
                        }
                    }
                    current = cdr;
                }
                _ => break,
            }
        }
        
        // Try global
        let mut current = self.global_env;
        loop {
            match self.lisp.get(current)? {
                Value::Nil => {
                    return Err(self.make_error(ErrorKind::UnboundVariable, name));
                }
                Value::Cons { car, cdr } => {
                    let binding = self.lisp.get(car)?;
                    if let Value::Cons { car: bound_name, cdr: _ } = binding {
                        if self.lisp.symbol_eq(bound_name, name)? {
                            self.lisp.set(car, Value::Cons { car: bound_name, cdr: value })?;
                            return Ok(value);
                        }
                    }
                    current = cdr;
                }
                _ => return Err(self.make_error(ErrorKind::Generic, name)),
            }
        }
    }
    
    /// Define in global environment
    pub fn define(&mut self, name: ArenaIndex, value: ArenaIndex) -> EvalResult {
        // Check if already defined and update
        let mut current = self.global_env;
        loop {
            match self.lisp.get(current)? {
                Value::Nil => {
                    // Not found, add new binding
                    self.global_env = self.env_extend(self.global_env, name, value)?;
                    return Ok(value);
                }
                Value::Cons { car, cdr } => {
                    let binding = self.lisp.get(car)?;
                    if let Value::Cons { car: bound_name, cdr: _ } = binding {
                        if self.lisp.symbol_eq(bound_name, name)? {
                            // Update existing
                            self.lisp.set(car, Value::Cons { car: bound_name, cdr: value })?;
                            return Ok(value);
                        }
                    }
                    current = cdr;
                }
                _ => return Err(self.make_error(ErrorKind::Generic, name)),
            }
        }
    }
    
    // ========================================================================
    // Main Evaluation - TCO via Trampoline
    // ========================================================================
    
    /// Evaluate an expression (entry point)
    pub fn eval(&mut self, expr: ArenaIndex) -> EvalResult {
        self.eval_in_env(expr, self.global_env)
    }
    
    /// Evaluate an expression in a given environment
    /// Uses a trampoline loop for TCO
    fn eval_in_env(&mut self, mut expr: ArenaIndex, mut env: ArenaIndex) -> EvalResult {
        loop {
            let val = self.lisp.get(expr)?;
            
            match val {
                // Self-evaluating values
                Value::Nil | Value::True | Value::False | 
                Value::Number(_) | Value::Char(_) | 
                Value::Builtin(_) | Value::Lambda { .. } | Value::Thunk { .. } => {
                    return Ok(expr);
                }
                
                // Symbol - variable lookup
                Value::Symbol { .. } => {
                    return self.env_lookup(env, expr);
                }
                
                // List - special form or function application
                Value::Cons { car, cdr } => {
                    let head = self.lisp.get(car)?;
                    
                    // Check for special forms
                    if let Value::Symbol { .. } = head {
                        // quote
                        if self.lisp.symbol_matches(car, "quote")? {
                            return Ok(self.lisp.car(cdr)?);
                        }
                        
                        // if - TCO in branches
                        if self.lisp.symbol_matches(car, "if")? {
                            let cond_expr = self.lisp.car(cdr)?;
                            let rest = self.lisp.cdr(cdr)?;
                            let then_expr = self.lisp.car(rest)?;
                            let else_rest = self.lisp.cdr(rest)?;
                            
                            // Evaluate condition (not tail position)
                            let cond_val = self.eval_in_env(cond_expr, env)?;
                            
                            // Choose branch - THIS is tail position (continue loop)
                            if !self.is_false(cond_val)? {
                                expr = then_expr;
                                continue;
                            } else if !self.lisp.get(else_rest)?.is_nil() {
                                expr = self.lisp.car(else_rest)?;
                                continue;
                            } else {
                                return self.lisp.nil().map_err(Into::into);
                            }
                        }
                        
                        // cond - TCO in final clause
                        if self.lisp.symbol_matches(car, "cond")? {
                            match self.eval_cond_tco(cdr, env)? {
                                TcoResult::Return(val) => return Ok(val),
                                TcoResult::TailCall { new_expr, new_env } => {
                                    expr = new_expr;
                                    env = new_env;
                                    continue;
                                }
                            }
                        }
                        
                        // lambda
                        if self.lisp.symbol_matches(car, "lambda")? {
                            return self.eval_lambda(cdr, env);
                        }
                        
                        // define
                        if self.lisp.symbol_matches(car, "define")? {
                            return self.eval_define(cdr, env);
                        }
                        
                        // let - TCO in body
                        if self.lisp.symbol_matches(car, "let")? {
                            let (new_expr, new_env) = self.eval_let_tco(cdr, env)?;
                            expr = new_expr;
                            env = new_env;
                            continue;
                        }
                        
                        // let* - TCO in body
                        if self.lisp.symbol_matches(car, "let*")? {
                            let (new_expr, new_env) = self.eval_let_star_tco(cdr, env)?;
                            expr = new_expr;
                            env = new_env;
                            continue;
                        }
                        
                        // begin - TCO in last expression
                        if self.lisp.symbol_matches(car, "begin")? {
                            match self.eval_begin_tco(cdr, env)? {
                                TcoResult::Return(val) => return Ok(val),
                                TcoResult::TailCall { new_expr, new_env } => {
                                    expr = new_expr;
                                    env = new_env;
                                    continue;
                                }
                            }
                        }
                        
                        // set!
                        if self.lisp.symbol_matches(car, "set!")? {
                            return self.eval_set(cdr, env);
                        }
                        
                        // and - short circuit
                        if self.lisp.symbol_matches(car, "and")? {
                            match self.eval_and_tco(cdr, env)? {
                                TcoResult::Return(val) => return Ok(val),
                                TcoResult::TailCall { new_expr, new_env } => {
                                    expr = new_expr;
                                    env = new_env;
                                    continue;
                                }
                            }
                        }
                        
                        // or - short circuit
                        if self.lisp.symbol_matches(car, "or")? {
                            match self.eval_or_tco(cdr, env)? {
                                TcoResult::Return(val) => return Ok(val),
                                TcoResult::TailCall { new_expr, new_env } => {
                                    expr = new_expr;
                                    env = new_env;
                                    continue;
                                }
                            }
                        }
                        
                        // delay - create thunk
                        if self.lisp.symbol_matches(car, "delay")? {
                            let delayed_expr = self.lisp.car(cdr)?;
                            return self.lisp.thunk(delayed_expr, env).map_err(Into::into);
                        }
                    }
                    
                    // Function application
                    self.push_frame(expr, car)?;
                    
                    let func = self.eval_in_env(car, env)?;
                    let args = self.eval_list(cdr, env)?;
                    
                    match self.lisp.get(func)? {
                        Value::Builtin(b) => {
                            let result = self.apply_builtin(b, args, expr)?;
                            self.pop_frame();
                            return Ok(result);
                        }
                        Value::Lambda { params, body, env: closure_env } => {
                            // TCO: set up for next iteration instead of recursing
                            let new_env = self.bind_params(params, args, closure_env, expr)?;
                            self.pop_frame();
                            expr = body;
                            env = new_env;
                            continue;
                        }
                        _ => {
                            self.pop_frame();
                            return Err(self.type_error(car, "procedure", self.lisp.get(func)?.type_name()));
                        }
                    }
                }
            }
        }
    }
    
    // ========================================================================
    // Special Form Helpers
    // ========================================================================
    
    /// Evaluate cond with TCO
    fn eval_cond_tco(&mut self, clauses: ArenaIndex, env: ArenaIndex) -> Result<TcoResult, EvalError> {
        let mut current = clauses;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => return Ok(TcoResult::Return(self.lisp.nil()?)),
                Value::Cons { car: clause, cdr: rest } => {
                    let test = self.lisp.car(clause)?;
                    let body = self.lisp.cdr(clause)?;
                    
                    // Check for 'else' clause
                    let is_else = self.lisp.symbol_matches(test, "else").unwrap_or(false);
                    
                    let test_result = if is_else {
                        self.lisp.true_val()?
                    } else {
                        self.eval_in_env(test, env)?
                    };
                    
                    if !self.is_false(test_result)? {
                        // Evaluate body - last expr is tail position
                        return self.eval_begin_tco(body, env);
                    }
                    
                    current = rest;
                }
                _ => return Err(self.make_error(ErrorKind::TypeError, clauses)),
            }
        }
    }
    
    /// Evaluate begin with TCO
    fn eval_begin_tco(&mut self, exprs: ArenaIndex, env: ArenaIndex) -> Result<TcoResult, EvalError> {
        let mut current = exprs;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => return Ok(TcoResult::Return(self.lisp.nil()?)),
                Value::Cons { car: expr, cdr: rest } => {
                    if self.lisp.get(rest)?.is_nil() {
                        // Last expression - tail position
                        return Ok(TcoResult::TailCall { new_expr: expr, new_env: env });
                    } else {
                        // Not last - evaluate and continue
                        self.eval_in_env(expr, env)?;
                        current = rest;
                    }
                }
                _ => return Err(self.make_error(ErrorKind::TypeError, current)),
            }
        }
    }
    
    /// Evaluate and with TCO
    fn eval_and_tco(&mut self, args: ArenaIndex, env: ArenaIndex) -> Result<TcoResult, EvalError> {
        let mut current = args;
        
        // Empty and returns #t
        if self.lisp.get(current)?.is_nil() {
            return Ok(TcoResult::Return(self.lisp.true_val()?));
        }
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => return Ok(TcoResult::Return(self.lisp.true_val()?)),
                Value::Cons { car: expr, cdr: rest } => {
                    if self.lisp.get(rest)?.is_nil() {
                        // Last expression - tail position
                        return Ok(TcoResult::TailCall { new_expr: expr, new_env: env });
                    } else {
                        let result = self.eval_in_env(expr, env)?;
                        if self.is_false(result)? {
                            return Ok(TcoResult::Return(self.lisp.false_val()?));
                        }
                        current = rest;
                    }
                }
                _ => return Err(self.make_error(ErrorKind::TypeError, current)),
            }
        }
    }
    
    /// Evaluate or with TCO
    fn eval_or_tco(&mut self, args: ArenaIndex, env: ArenaIndex) -> Result<TcoResult, EvalError> {
        let mut current = args;
        
        // Empty or returns #f
        if self.lisp.get(current)?.is_nil() {
            return Ok(TcoResult::Return(self.lisp.false_val()?));
        }
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => return Ok(TcoResult::Return(self.lisp.false_val()?)),
                Value::Cons { car: expr, cdr: rest } => {
                    if self.lisp.get(rest)?.is_nil() {
                        // Last expression - tail position
                        return Ok(TcoResult::TailCall { new_expr: expr, new_env: env });
                    } else {
                        let result = self.eval_in_env(expr, env)?;
                        if !self.is_false(result)? {
                            return Ok(TcoResult::Return(result));
                        }
                        current = rest;
                    }
                }
                _ => return Err(self.make_error(ErrorKind::TypeError, current)),
            }
        }
    }
    
    /// Evaluate let with TCO in body
    fn eval_let_tco(&mut self, args: ArenaIndex, env: ArenaIndex) -> Result<(ArenaIndex, ArenaIndex), EvalError> {
        let bindings = self.lisp.car(args)?;
        let body = self.lisp.cdr(args)?;
        
        // Extend environment with all bindings
        let mut new_env = env;
        let mut current = bindings;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => break,
                Value::Cons { car: binding, cdr: rest } => {
                    let name = self.lisp.car(binding)?;
                    let value_expr = self.lisp.car(self.lisp.cdr(binding)?)?;
                    let value = self.eval_in_env(value_expr, env)?; // Use original env
                    new_env = self.env_extend(new_env, name, value)?;
                    current = rest;
                }
                _ => return Err(self.make_error(ErrorKind::TypeError, bindings)),
            }
        }
        
        // Body becomes a begin block for TCO
        let body_expr = if self.lisp.get(self.lisp.cdr(body)?)?.is_nil() {
            self.lisp.car(body)?
        } else {
            // Wrap in begin
            let begin = self.lisp.symbol("begin")?;
            self.lisp.cons(begin, body)?
        };
        
        Ok((body_expr, new_env))
    }
    
    /// Evaluate let* with TCO in body
    fn eval_let_star_tco(&mut self, args: ArenaIndex, env: ArenaIndex) -> Result<(ArenaIndex, ArenaIndex), EvalError> {
        let bindings = self.lisp.car(args)?;
        let body = self.lisp.cdr(args)?;
        
        // Extend environment sequentially
        let mut new_env = env;
        let mut current = bindings;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => break,
                Value::Cons { car: binding, cdr: rest } => {
                    let name = self.lisp.car(binding)?;
                    let value_expr = self.lisp.car(self.lisp.cdr(binding)?)?;
                    let value = self.eval_in_env(value_expr, new_env)?; // Use NEW env
                    new_env = self.env_extend(new_env, name, value)?;
                    current = rest;
                }
                _ => return Err(self.make_error(ErrorKind::TypeError, bindings)),
            }
        }
        
        let body_expr = if self.lisp.get(self.lisp.cdr(body)?)?.is_nil() {
            self.lisp.car(body)?
        } else {
            let begin = self.lisp.symbol("begin")?;
            self.lisp.cons(begin, body)?
        };
        
        Ok((body_expr, new_env))
    }
    
    /// Evaluate lambda
    fn eval_lambda(&mut self, args: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let params = self.lisp.car(args)?;
        let body_list = self.lisp.cdr(args)?;
        
        // Wrap body in begin if multiple expressions
        let body = if self.lisp.get(self.lisp.cdr(body_list)?)?.is_nil() {
            self.lisp.car(body_list)?
        } else {
            let begin = self.lisp.symbol("begin")?;
            self.lisp.cons(begin, body_list)?
        };
        
        self.lisp.lambda(params, body, env).map_err(Into::into)
    }
    
    /// Evaluate define
    fn eval_define(&mut self, args: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let first = self.lisp.car(args)?;
        let rest = self.lisp.cdr(args)?;
        
        match self.lisp.get(first)? {
            // (define name value)
            Value::Symbol { .. } => {
                let value_expr = self.lisp.car(rest)?;
                let value = self.eval_in_env(value_expr, env)?;
                self.define(first, value)
            }
            // (define (name params...) body...) -> (define name (lambda (params...) body...))
            Value::Cons { car: name, cdr: params } => {
                let body_list = rest;
                let body = if self.lisp.get(self.lisp.cdr(body_list)?)?.is_nil() {
                    self.lisp.car(body_list)?
                } else {
                    let begin = self.lisp.symbol("begin")?;
                    self.lisp.cons(begin, body_list)?
                };
                let lambda = self.lisp.lambda(params, body, env)?;
                self.define(name, lambda)
            }
            _ => Err(self.type_error(first, "symbol or list", self.lisp.get(first)?.type_name())),
        }
    }
    
    /// Evaluate set!
    fn eval_set(&mut self, args: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let name = self.lisp.car(args)?;
        let value_expr = self.lisp.car(self.lisp.cdr(args)?)?;
        let value = self.eval_in_env(value_expr, env)?;
        self.env_set(env, name, value)
    }
    
    // ========================================================================
    // Helpers
    // ========================================================================
    
    /// Evaluate a list of expressions (for function arguments)
    fn eval_list(&mut self, list: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let val = self.lisp.get(list)?;
        
        match val {
            Value::Nil => self.lisp.nil().map_err(Into::into),
            Value::Cons { car, cdr } => {
                let head = self.eval_in_env(car, env)?;
                let tail = self.eval_list(cdr, env)?;
                self.lisp.cons(head, tail).map_err(Into::into)
            }
            _ => Err(self.make_error(ErrorKind::TypeError, list)),
        }
    }
    
    /// Bind parameters to arguments
    fn bind_params(&self, params: ArenaIndex, args: ArenaIndex, env: ArenaIndex, call_expr: ArenaIndex) -> EvalResult {
        let mut new_env = env;
        let mut params_cur = params;
        let mut args_cur = args;
        
        loop {
            let p = self.lisp.get(params_cur)?;
            let a = self.lisp.get(args_cur)?;
            
            match (p, a) {
                (Value::Nil, Value::Nil) => break,
                (Value::Cons { car: param, cdr: prest }, 
                 Value::Cons { car: arg, cdr: arest }) => {
                    new_env = self.env_extend(new_env, param, arg)?;
                    params_cur = prest;
                    args_cur = arest;
                }
                (Value::Nil, Value::Cons { .. }) => {
                    // Too many arguments
                    let expected = self.count_list(params)?;
                    let got = self.count_list(args)?;
                    return Err(self.arg_error(call_expr, expected, got));
                }
                (Value::Cons { .. }, Value::Nil) => {
                    // Too few arguments
                    let expected = self.count_list(params)?;
                    let got = self.count_list(args)?;
                    return Err(self.arg_error(call_expr, expected, got));
                }
                // Rest parameter (symbol instead of nil at end)
                (Value::Symbol { .. }, _) => {
                    // Bind remaining args to rest parameter
                    new_env = self.env_extend(new_env, params_cur, args_cur)?;
                    break;
                }
                _ => return Err(self.make_error(ErrorKind::TypeError, params_cur)),
            }
        }
        
        Ok(new_env)
    }
    
    /// Count elements in a list
    fn count_list(&self, mut list: ArenaIndex) -> Result<usize, EvalError> {
        let mut count = 0;
        loop {
            match self.lisp.get(list)? {
                Value::Nil => return Ok(count),
                Value::Cons { cdr, .. } => {
                    count += 1;
                    list = cdr;
                }
                _ => return Ok(count), // Rest parameter
            }
        }
    }
    
    /// Check if a value is false (ONLY #f is false)
    fn is_false(&self, val: ArenaIndex) -> Result<bool, EvalError> {
        Ok(self.lisp.get(val)?.is_false())
    }
    
    /// Force a thunk (lazy evaluation)
    fn force(&mut self, idx: ArenaIndex) -> EvalResult {
        let val = self.lisp.get(idx)?;
        
        match val {
            Value::Thunk { expr, env, cached } => {
                if !cached.is_null() {
                    // Already evaluated - return cached result
                    return Ok(cached);
                }
                
                // Evaluate the thunk
                let result = self.eval_in_env(expr, env)?;
                
                // Cache the result (memoization)
                self.lisp.set(idx, Value::Thunk { expr, env, cached: result })?;
                
                Ok(result)
            }
            _ => Ok(idx), // Not a thunk, return as-is
        }
    }
    
    // ========================================================================
    // Built-in Functions
    // ========================================================================
    
    fn apply_builtin(&mut self, builtin: Builtin, args: ArenaIndex, call_expr: ArenaIndex) -> EvalResult {
        match builtin {
            Builtin::Car => {
                let arg = self.lisp.car(args)?;
                match self.lisp.get(arg)? {
                    Value::Cons { car, .. } => Ok(car),
                    Value::Nil => self.lisp.nil().map_err(Into::into),
                    _ => Err(self.type_error(call_expr, "pair", self.lisp.get(arg)?.type_name())),
                }
            }
            
            Builtin::Cdr => {
                let arg = self.lisp.car(args)?;
                match self.lisp.get(arg)? {
                    Value::Cons { cdr, .. } => Ok(cdr),
                    Value::Nil => self.lisp.nil().map_err(Into::into),
                    _ => Err(self.type_error(call_expr, "pair", self.lisp.get(arg)?.type_name())),
                }
            }
            
            Builtin::Cons => {
                let a = self.lisp.car(args)?;
                let rest = self.lisp.cdr(args)?;
                let b = self.lisp.car(rest)?;
                self.lisp.cons(a, b).map_err(Into::into)
            }
            
            Builtin::List => Ok(args),
            
            Builtin::SetCar => {
                let pair = self.lisp.car(args)?;
                let new_val = self.lisp.car(self.lisp.cdr(args)?)?;
                
                match self.lisp.get(pair)? {
                    Value::Cons { cdr, .. } => {
                        self.lisp.set(pair, Value::Cons { car: new_val, cdr })?;
                        Ok(new_val)
                    }
                    _ => Err(self.type_error(call_expr, "pair", self.lisp.get(pair)?.type_name())),
                }
            }
            
            Builtin::SetCdr => {
                let pair = self.lisp.car(args)?;
                let new_val = self.lisp.car(self.lisp.cdr(args)?)?;
                
                match self.lisp.get(pair)? {
                    Value::Cons { car, .. } => {
                        self.lisp.set(pair, Value::Cons { car, cdr: new_val })?;
                        Ok(new_val)
                    }
                    _ => Err(self.type_error(call_expr, "pair", self.lisp.get(pair)?.type_name())),
                }
            }
            
            Builtin::Atom => {
                let arg = self.lisp.car(args)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_atom()).map_err(Into::into)
            }
            
            Builtin::Eq => {
                let a = self.lisp.car(args)?;
                let rest = self.lisp.cdr(args)?;
                let b = self.lisp.car(rest)?;
                
                let val_a = self.lisp.get(a)?;
                let val_b = self.lisp.get(b)?;
                
                let eq = match (val_a, val_b) {
                    (Value::Nil, Value::Nil) => true,
                    (Value::True, Value::True) => true,
                    (Value::False, Value::False) => true,
                    (Value::Number(x), Value::Number(y)) => x == y,
                    (Value::Char(x), Value::Char(y)) => x == y,
                    (Value::Symbol { .. }, Value::Symbol { .. }) => self.lisp.symbol_eq(a, b)?,
                    _ => a == b, // Same index
                };
                
                self.lisp.boolean(eq).map_err(Into::into)
            }
            
            Builtin::Null => {
                let arg = self.lisp.car(args)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_nil()).map_err(Into::into)
            }
            
            Builtin::Pairp => {
                let arg = self.lisp.car(args)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_cons()).map_err(Into::into)
            }
            
            Builtin::Numberp => {
                let arg = self.lisp.car(args)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_number()).map_err(Into::into)
            }
            
            Builtin::Booleanp => {
                let arg = self.lisp.car(args)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_boolean()).map_err(Into::into)
            }
            
            Builtin::Procedurep => {
                let arg = self.lisp.car(args)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_procedure()).map_err(Into::into)
            }
            
            Builtin::Symbolp => {
                let arg = self.lisp.car(args)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_symbol()).map_err(Into::into)
            }
            
            Builtin::Force => {
                let arg = self.lisp.car(args)?;
                self.force(arg)
            }
            
            Builtin::Promisep => {
                let arg = self.lisp.car(args)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_thunk()).map_err(Into::into)
            }
            
            Builtin::Not => {
                let arg = self.lisp.car(args)?;
                self.lisp.boolean(self.is_false(arg)?).map_err(Into::into)
            }
            
            Builtin::Add => self.numeric_fold(args, 0, |a, b| a.checked_add(b), call_expr),
            
            Builtin::Sub => {
                let first = self.get_number(self.lisp.car(args)?, call_expr)?;
                let rest = self.lisp.cdr(args)?;
                if self.lisp.get(rest)?.is_nil() {
                    // Unary minus
                    self.lisp.number(-first).map_err(Into::into)
                } else {
                    self.numeric_fold_start(rest, first, |a, b| a.checked_sub(b), call_expr)
                }
            }
            
            Builtin::Mul => self.numeric_fold(args, 1, |a, b| a.checked_mul(b), call_expr),
            
            Builtin::Div => {
                let first = self.get_number(self.lisp.car(args)?, call_expr)?;
                let rest = self.lisp.cdr(args)?;
                self.numeric_fold_start(rest, first, |a, b| {
                    if b == 0 { None } else { a.checked_div(b) }
                }, call_expr)
            }
            
            Builtin::Mod => {
                let a = self.get_number(self.lisp.car(args)?, call_expr)?;
                let b = self.get_number(self.lisp.car(self.lisp.cdr(args)?)?, call_expr)?;
                if b == 0 {
                    return Err(self.make_error(ErrorKind::DivisionByZero, call_expr));
                }
                self.lisp.number(a % b).map_err(Into::into)
            }
            
            Builtin::Lt => self.compare(args, |a, b| a < b, call_expr),
            Builtin::Gt => self.compare(args, |a, b| a > b, call_expr),
            Builtin::Le => self.compare(args, |a, b| a <= b, call_expr),
            Builtin::Ge => self.compare(args, |a, b| a >= b, call_expr),
            Builtin::NumEq => self.compare(args, |a, b| a == b, call_expr),
            
            Builtin::Print | Builtin::Display => {
                // These are no-ops in no_std eval, handled by REPL
                Ok(self.lisp.car(args)?)
            }
            
            Builtin::Newline => {
                self.lisp.nil().map_err(Into::into)
            }
            
            Builtin::Error => {
                let msg = self.lisp.car(args)?;
                Err(self.make_error(ErrorKind::UserError, msg))
            }
        }
    }
    
    fn get_number(&self, idx: ArenaIndex, call_expr: ArenaIndex) -> Result<i64, EvalError> {
        match self.lisp.get(idx)? {
            Value::Number(n) => Ok(n),
            v => Err(self.type_error(call_expr, "number", v.type_name())),
        }
    }
    
    fn numeric_fold<F>(&self, args: ArenaIndex, init: i64, f: F, call_expr: ArenaIndex) -> EvalResult
    where F: Fn(i64, i64) -> Option<i64>
    {
        self.numeric_fold_start(args, init, f, call_expr)
    }
    
    fn numeric_fold_start<F>(&self, args: ArenaIndex, mut acc: i64, f: F, call_expr: ArenaIndex) -> EvalResult
    where F: Fn(i64, i64) -> Option<i64>
    {
        let mut current = args;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => return self.lisp.number(acc).map_err(Into::into),
                Value::Cons { car, cdr } => {
                    let n = self.get_number(car, call_expr)?;
                    acc = f(acc, n).ok_or_else(|| self.make_error(ErrorKind::DivisionByZero, call_expr))?;
                    current = cdr;
                }
                _ => return Err(self.make_error(ErrorKind::TypeError, current)),
            }
        }
    }
    
    fn compare<F>(&self, args: ArenaIndex, cmp: F, call_expr: ArenaIndex) -> EvalResult
    where F: Fn(i64, i64) -> bool
    {
        let a = self.get_number(self.lisp.car(args)?, call_expr)?;
        let b = self.get_number(self.lisp.car(self.lisp.cdr(args)?)?, call_expr)?;
        self.lisp.boolean(cmp(a, b)).map_err(Into::into)
    }
    
    // ========================================================================
    // Convenience
    // ========================================================================
    
    /// Evaluate a string
    pub fn eval_str(&mut self, input: &str) -> EvalResult {
        let expr = parse(self.lisp, input)?;
        self.eval(expr)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    fn eval_to_num<const N: usize>(lisp: &Lisp<N>, eval: &mut Evaluator<N>, input: &str) -> i64 {
        let result = eval.eval_str(input).unwrap();
        lisp.get(result).unwrap().as_number().unwrap()
    }
    
    fn eval_is_true<const N: usize>(lisp: &Lisp<N>, eval: &mut Evaluator<N>, input: &str) -> bool {
        let result = eval.eval_str(input).unwrap();
        lisp.get(result).unwrap().is_true()
    }
    
    fn eval_is_false<const N: usize>(lisp: &Lisp<N>, eval: &mut Evaluator<N>, input: &str) -> bool {
        let result = eval.eval_str(input).unwrap();
        lisp.get(result).unwrap().is_false()
    }
    
    #[test]
    fn test_eval_number() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "42"), 42);
        assert_eq!(eval_to_num(&lisp, &mut eval, "-10"), -10);
    }
    
    #[test]
    fn test_eval_booleans() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert!(eval_is_true(&lisp, &mut eval, "#t"));
        assert!(eval_is_false(&lisp, &mut eval, "#f"));
        assert!(eval_is_true(&lisp, &mut eval, "true"));
        assert!(eval_is_false(&lisp, &mut eval, "false"));
    }
    
    #[test]
    fn test_nil_is_truthy() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // nil/'() is NOT false - only #f is false
        assert_eq!(eval_to_num(&lisp, &mut eval, "(if nil 1 2)"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(if '() 1 2)"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(if 0 1 2)"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(if #f 1 2)"), 2);
    }
    
    #[test]
    fn test_eval_arithmetic() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(+ 1 2)"), 3);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(- 10 3)"), 7);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(* 4 5)"), 20);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(/ 20 4)"), 5);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(+ 1 2 3 4)"), 10);
    }
    
    #[test]
    fn test_eval_quote() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        let result = eval.eval_str("'hello").unwrap();
        assert!(lisp.symbol_matches(result, "hello").unwrap());
    }
    
    #[test]
    fn test_eval_if() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(if #t 1 2)"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(if #f 1 2)"), 2);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(if (< 1 2) 10 20)"), 10);
    }
    
    #[test]
    fn test_eval_define() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define x 42)").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "x"), 42);
    }
    
    #[test]
    fn test_eval_lambda() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "((lambda (x) (+ x 1)) 5)"), 6);
    }
    
    #[test]
    fn test_eval_define_function() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (square x) (* x x))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(square 5)"), 25);
    }
    
    #[test]
    fn test_eval_let() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(let ((x 10) (y 20)) (+ x y))"), 30);
    }
    
    #[test]
    fn test_eval_let_star() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // let* allows sequential binding
        assert_eq!(eval_to_num(&lisp, &mut eval, "(let* ((x 10) (y (+ x 5))) (+ x y))"), 25);
    }
    
    #[test]
    fn test_tco_recursion() {
        let lisp: Lisp<5000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // This would stack overflow without TCO
        eval.eval_str("(define (sum-to n acc) (if (= n 0) acc (sum-to (- n 1) (+ acc n))))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(sum-to 100 0)"), 5050);
    }
    
    #[test]
    fn test_eval_recursion() {
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (fact n) (if (= n 0) 1 (* n (fact (- n 1)))))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(fact 5)"), 120);
    }
    
    #[test]
    fn test_set_car_cdr() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define x (cons 1 2))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car x)"), 1);
        
        eval.eval_str("(set-car! x 100)").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car x)"), 100);
        
        eval.eval_str("(set-cdr! x 200)").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(cdr x)"), 200);
    }
    
    #[test]
    fn test_delay_force() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Create a thunk
        eval.eval_str("(define lazy-val (delay (+ 1 2)))").unwrap();
        
        // Check it's a promise
        assert!(eval_is_true(&lisp, &mut eval, "(promise? lazy-val)"));
        
        // Force it
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force lazy-val)"), 3);
        
        // Force again - should return cached value
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force lazy-val)"), 3);
    }
    
    #[test]
    fn test_lazy_memoization() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Use mutation to verify memoization
        eval.eval_str("(define counter 0)").unwrap();
        eval.eval_str("(define lazy-inc (delay (begin (set! counter (+ counter 1)) counter)))").unwrap();
        
        // First force - increments counter
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force lazy-inc)"), 1);
        
        // Second force - should return cached value, not increment again
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force lazy-inc)"), 1);
        
        // Counter should still be 1
        assert_eq!(eval_to_num(&lisp, &mut eval, "counter"), 1);
    }
    
    #[test]
    fn test_thunk_force_non_thunk() {
        // Force on a non-thunk should return the value as-is
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force 42)"), 42);
        assert!(eval_is_true(&lisp, &mut eval, "(force #t)"));
        assert!(eval_is_false(&lisp, &mut eval, "(force #f)"));
    }
    
    #[test]
    fn test_thunk_nested_delay() {
        // Nested delays - each force unwraps one layer
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define double-lazy (delay (delay 42)))").unwrap();
        
        // First force returns inner thunk
        assert!(eval_is_true(&lisp, &mut eval, "(promise? (force double-lazy))"));
        
        // Force twice to get the value
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force (force double-lazy))"), 42);
    }
    
    #[test]
    fn test_thunk_captures_environment() {
        // Thunks should capture their lexical environment
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (make-lazy-adder x) (delay (+ x 10)))").unwrap();
        eval.eval_str("(define lazy-add-5 (make-lazy-adder 5))").unwrap();
        eval.eval_str("(define lazy-add-20 (make-lazy-adder 20))").unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force lazy-add-5)"), 15);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force lazy-add-20)"), 30);
    }
    
    #[test]
    fn test_thunk_in_data_structures() {
        // Thunks can be stored in data structures
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define lazy-pair (cons (delay 1) (delay 2)))").unwrap();
        
        assert!(eval_is_true(&lisp, &mut eval, "(promise? (car lazy-pair))"));
        assert!(eval_is_true(&lisp, &mut eval, "(promise? (cdr lazy-pair))"));
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force (car lazy-pair))"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force (cdr lazy-pair))"), 2);
    }
    
    #[test]
    fn test_thunk_conditional_evaluation() {
        // Thunks allow conditional evaluation of expensive operations
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define expensive-counter 0)").unwrap();
        eval.eval_str("(define (expensive-op) (set! expensive-counter (+ expensive-counter 1)) 999)").unwrap();
        
        // Create thunks for both branches
        eval.eval_str("(define then-branch (delay (expensive-op)))").unwrap();
        eval.eval_str("(define else-branch (delay (expensive-op)))").unwrap();
        
        // Only force one branch based on condition
        eval.eval_str("(define (lazy-if cond then else) (if cond (force then) (force else)))").unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(lazy-if #t then-branch else-branch)"), 999);
        
        // Only one expensive operation should have been executed
        assert_eq!(eval_to_num(&lisp, &mut eval, "expensive-counter"), 1);
    }
    
    #[test]
    fn test_thunk_multiple_forces_same_result() {
        // Multiple forces should always return the same result
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define counter 0)").unwrap();
        eval.eval_str("(define thunk (delay (begin (set! counter (+ counter 1)) (* counter 10))))").unwrap();
        
        // Force multiple times
        let r1 = eval_to_num(&lisp, &mut eval, "(force thunk)");
        let r2 = eval_to_num(&lisp, &mut eval, "(force thunk)");
        let r3 = eval_to_num(&lisp, &mut eval, "(force thunk)");
        
        assert_eq!(r1, 10);
        assert_eq!(r2, 10);  // Should be same, not 20
        assert_eq!(r3, 10);  // Should be same, not 30
        
        // Counter should only have been incremented once
        assert_eq!(eval_to_num(&lisp, &mut eval, "counter"), 1);
    }
    
    #[test]
    fn test_thunk_with_closures() {
        // Thunks work correctly with closures
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (make-counter) (let ((n 0)) (delay (begin (set! n (+ n 1)) n))))").unwrap();
        eval.eval_str("(define counter1 (make-counter))").unwrap();
        eval.eval_str("(define counter2 (make-counter))").unwrap();
        
        // Each counter has its own environment
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force counter1)"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force counter1)"), 1);  // Memoized
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force counter2)"), 1);  // Independent
    }
    
    #[test]
    fn test_thunk_promise_predicate() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // promise? returns true only for thunks
        assert!(eval_is_true(&lisp, &mut eval, "(promise? (delay 42))"));
        assert!(eval_is_false(&lisp, &mut eval, "(promise? 42)"));
        assert!(eval_is_false(&lisp, &mut eval, "(promise? '())"));
        assert!(eval_is_false(&lisp, &mut eval, "(promise? #t)"));
        assert!(eval_is_false(&lisp, &mut eval, "(promise? (lambda (x) x))"));
        assert!(eval_is_false(&lisp, &mut eval, "(promise? +)"));
    }
    
    #[test]
    fn test_thunk_delay_does_not_evaluate() {
        // delay should not evaluate its expression until forced
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define evaluated #f)").unwrap();
        eval.eval_str("(define lazy-val (delay (begin (set! evaluated #t) 42)))").unwrap();
        
        // evaluated should still be false
        assert!(eval_is_false(&lisp, &mut eval, "evaluated"));
        
        // Now force it
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force lazy-val)"), 42);
        
        // Now evaluated should be true
        assert!(eval_is_true(&lisp, &mut eval, "evaluated"));
    }
    
    #[test]
    fn test_thunk_lazy_list_operations() {
        // Implement simple lazy list operations
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Lazy cons: head is eager, tail is lazy
        eval.eval_str("(define (lazy-cons head tail-thunk) (cons head tail-thunk))").unwrap();
        eval.eval_str("(define (lazy-car stream) (car stream))").unwrap();
        eval.eval_str("(define (lazy-cdr stream) (force (cdr stream)))").unwrap();
        
        // Create a lazy list: (1 2 3)
        eval.eval_str("(define lazy-list (lazy-cons 1 (delay (lazy-cons 2 (delay (lazy-cons 3 (delay '())))))))").unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(lazy-car lazy-list)"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(lazy-car (lazy-cdr lazy-list))"), 2);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(lazy-car (lazy-cdr (lazy-cdr lazy-list)))"), 3);
    }
    
    #[test]
    fn test_thunk_avoids_infinite_computation() {
        // Without laziness, this would loop forever
        // With laziness, we only compute what we need
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Define a "take" that works with our lazy streams
        eval.eval_str("(define (lazy-cons h t) (cons h t))").unwrap();
        eval.eval_str("(define (lazy-car s) (car s))").unwrap();
        eval.eval_str("(define (lazy-cdr s) (force (cdr s)))").unwrap();
        
        // Infinite stream of ones (well, would be infinite if we kept forcing)
        eval.eval_str("(define ones (lazy-cons 1 (delay ones)))").unwrap();
        
        // We can safely get elements without infinite loop
        assert_eq!(eval_to_num(&lisp, &mut eval, "(lazy-car ones)"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(lazy-car (lazy-cdr ones))"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(lazy-car (lazy-cdr (lazy-cdr ones)))"), 1);
    }
    
    #[test]
    fn test_thunk_gc_preserves_thunks() {
        // GC should preserve thunks that are still reachable
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define my-thunk (delay (+ 100 200)))").unwrap();
        
        // Create some garbage
        for _ in 0..50 {
            eval.eval_str("(+ 1 2)").unwrap();
        }
        
        // Run GC
        eval.gc();
        
        // Thunk should still work
        assert!(eval_is_true(&lisp, &mut eval, "(promise? my-thunk)"));
        assert_eq!(eval_to_num(&lisp, &mut eval, "(force my-thunk)"), 300);
    }
    
    #[test]
    fn test_thunk_gc_preserves_forced_value() {
        // GC should preserve the cached value in a forced thunk
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define my-thunk (delay (cons 1 2)))").unwrap();
        
        // Force it to cache the result
        eval.eval_str("(force my-thunk)").unwrap();
        
        // Create garbage and run GC
        for _ in 0..50 {
            eval.eval_str("(+ 1 2)").unwrap();
        }
        eval.gc();
        
        // Forced value should still be accessible
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (force my-thunk))"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(cdr (force my-thunk))"), 2);
    }
    
    #[test]
    fn test_thunk_expression_only_evaluated_once() {
        // Even if expression has side effects, it should only run once
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define call-log '())").unwrap();
        eval.eval_str("(define (log-and-return x) (set! call-log (cons x call-log)) x)").unwrap();
        
        eval.eval_str("(define lazy-42 (delay (log-and-return 42)))").unwrap();
        
        // Force multiple times
        eval.eval_str("(force lazy-42)").unwrap();
        eval.eval_str("(force lazy-42)").unwrap();
        eval.eval_str("(force lazy-42)").unwrap();
        
        // log-and-return should only have been called once
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car call-log)"), 42);
        assert!(eval_is_true(&lisp, &mut eval, "(null? (cdr call-log))"));
    }
    
    #[test]
    fn test_predicates() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert!(eval_is_true(&lisp, &mut eval, "(null? '())"));
        assert!(eval_is_true(&lisp, &mut eval, "(null? nil)"));
        assert!(eval_is_false(&lisp, &mut eval, "(null? '(1))"));
        
        assert!(eval_is_true(&lisp, &mut eval, "(pair? '(1 . 2))"));
        assert!(eval_is_false(&lisp, &mut eval, "(pair? 42)"));
        
        assert!(eval_is_true(&lisp, &mut eval, "(number? 42)"));
        assert!(eval_is_false(&lisp, &mut eval, "(number? 'x)"));
        
        assert!(eval_is_true(&lisp, &mut eval, "(boolean? #t)"));
        assert!(eval_is_true(&lisp, &mut eval, "(boolean? #f)"));
        assert!(eval_is_false(&lisp, &mut eval, "(boolean? 1)"));
        
        assert!(eval_is_true(&lisp, &mut eval, "(symbol? 'x)"));
        assert!(eval_is_false(&lisp, &mut eval, "(symbol? 42)"));
        
        assert!(eval_is_true(&lisp, &mut eval, "(procedure? +)"));
        assert!(eval_is_true(&lisp, &mut eval, "(procedure? (lambda (x) x))"));
        assert!(eval_is_false(&lisp, &mut eval, "(procedure? 42)"));
    }
    
    #[test]
    fn test_not() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert!(eval_is_true(&lisp, &mut eval, "(not #f)"));
        assert!(eval_is_false(&lisp, &mut eval, "(not #t)"));
        assert!(eval_is_false(&lisp, &mut eval, "(not nil)")); // nil is truthy!
        assert!(eval_is_false(&lisp, &mut eval, "(not 0)"));   // 0 is truthy!
    }
    
    #[test]
    fn test_cond() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        let result = eval_to_num(&lisp, &mut eval, 
            "(cond ((< 5 3) 1) ((> 5 3) 2) (else 3))");
        assert_eq!(result, 2);
    }
    
    #[test]
    fn test_and_or() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert!(eval_is_true(&lisp, &mut eval, "(and #t #t)"));
        assert!(eval_is_false(&lisp, &mut eval, "(and #t #f)"));
        assert!(eval_is_true(&lisp, &mut eval, "(and)"));  // Empty and is true
        
        assert!(eval_is_true(&lisp, &mut eval, "(or #f #t)"));
        assert!(eval_is_false(&lisp, &mut eval, "(or #f #f)"));
        assert!(eval_is_false(&lisp, &mut eval, "(or)"));  // Empty or is false
    }
    
    #[test]
    fn test_gc_during_eval() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define x 42)").unwrap();
        let _stats = eval.gc();
        assert_eq!(eval_to_num(&lisp, &mut eval, "x"), 42);
    }
}
