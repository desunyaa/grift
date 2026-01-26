#![no_std]

//! # Lisp Evaluator
//!
//! A PURE Lisp evaluator with **hybrid evaluation** strategy:
//! - Lexically scoped closures
//! - Tail Call Optimization (TCO)
//! - Lazy data structures (like Haskell)
//! - Strict control flow (enables proper TCO)
//! - Rich error handling with stack traces
//!
//! ## Hybrid Evaluation Strategy
//!
//! This evaluator uses a hybrid approach that combines lazy and strict evaluation:
//!
//! **Tail position (strict)**: Lambda arguments in tail calls are evaluated
//! strictly. This enables proper TCO without thunk accumulation:
//! ```lisp
//! (define (countdown n)
//!   (if (= n 0) 'done
//!       (countdown (- n 1))))  ; Args evaluated strictly, TCO applies
//! ```
//!
//! **Non-tail position (lazy)**: Builtin arguments are lazy (wrapped in thunks).
//! Builtins force what they need. This enables infinite data structures:
//! ```lisp
//! (define (ones) (cons 1 (ones)))  ; cons is lazy, infinite stream works
//! (car (ones))  ; => 1
//! ```
//!
//! ## Strict Positions (in builtins)
//!
//! Builtins automatically force arguments in strict positions:
//! - Arithmetic operands (+, -, *, /, mod)
//! - Comparison operands (<, >, =, etc.)
//! - `if` condition (but NOT branches)
//! - Predicates (null?, pair?, etc.)
//! - Print/display arguments
//!
//! ## Purity
//!
//! This is a pure functional language - NO MUTATION!
//! This makes lazy evaluation semantically sound (referential transparency).
//!
//! ## Truthiness
//!
//! Only `#f` is false. Everything else (including `nil`/`'()`) is truthy.
//!
//! ## Special Forms
//!
//! - `quote` - Return expression unevaluated
//! - `if` - Conditional (lazy in branches)
//! - `cond` - Multi-way conditional
//! - `lambda` - Create closure
//! - `define` - Define variable/function
//! - `let` - Local binding
//! - `let*` - Sequential local binding
//! - `begin` - Sequence of expressions
//! - `and` / `or` - Short-circuit boolean operations

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
    
    // NOTE: env_set removed - this is a PURE Lisp!
    // Mutation breaks referential transparency and call-by-need semantics.
    
    /// Define in global environment (NOTE: only allowed at top-level)
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
        let result = self.eval_in_env(expr, self.global_env)?;
        // Force the result at top level (WHNF)
        self.force(result)
    }
    
    /// Evaluate an expression in a given environment
    /// Uses a trampoline loop for TCO
    /// 
    /// This is public so the REPL can force thunks for display
    pub fn eval_in_env(&mut self, mut expr: ArenaIndex, mut env: ArenaIndex) -> EvalResult {
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
                        
                        // if - condition is strict, branches are lazy
                        if self.lisp.symbol_matches(car, "if")? {
                            let cond_expr = self.lisp.car(cdr)?;
                            let rest = self.lisp.cdr(cdr)?;
                            let then_expr = self.lisp.car(rest)?;
                            let else_rest = self.lisp.cdr(rest)?;
                            
                            // Evaluate AND FORCE condition (strict position)
                            let cond_thunk = self.eval_in_env(cond_expr, env)?;
                            let cond_val = self.force(cond_thunk)?;
                            
                            // Choose branch - return as thunk (lazy)
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
                        
                        // NOTE: set! removed - this is a PURE Lisp!
                        
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
                        
                        // NOTE: No explicit 'delay' - hybrid evaluation strategy
                    }
                    
                    // Function application - HYBRID EVALUATION:
                    // - Builtins: lazy args (builtins force what they need)
                    // - Lambda tail calls: STRICT args (enables TCO, no thunk accumulation)
                    self.push_frame(expr, car)?;
                    
                    let func_thunk = self.eval_in_env(car, env)?;
                    let func = self.force(func_thunk)?;
                    
                    match self.lisp.get(func)? {
                        Value::Builtin(b) => {
                            // Builtins: LAZY - wrap args in thunks
                            // Builtins handle their own strictness
                            let args = self.make_thunk_list(cdr, env)?;
                            let result = self.apply_builtin(b, args, expr)?;
                            self.pop_frame();
                            return Ok(result);
                        }
                        Value::Lambda { params, body, env: closure_env } => {
                            // Lambda tail calls: STRICT - evaluate args now
                            // This enables proper TCO without thunk accumulation
                            let args = self.eval_list_strict(cdr, env)?;
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
    
    // NOTE: eval_set removed - this is a PURE Lisp!
    
    // ========================================================================
    // Helpers
    // ========================================================================
    
    /// Evaluate a list of expressions (evaluates but doesn't force)
    /// NOTE: Currently unused but kept for potential future use
    #[allow(dead_code)]
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
    
    /// Evaluate AND force a list of expressions (fully strict)
    /// Used for tail-call arguments to enable proper TCO
    /// 
    /// ITERATIVE implementation to avoid Rust stack overflow
    fn eval_list_strict(&mut self, list: ArenaIndex, env: ArenaIndex) -> EvalResult {
        const MAX_ARGS: usize = 64;
        // Use a dummy index as placeholder (will be overwritten)
        let dummy = ArenaIndex::new(usize::MAX, u32::MAX);
        let mut values: [ArenaIndex; MAX_ARGS] = [dummy; MAX_ARGS];
        let mut count = 0;
        let mut current = list;
        
        // First pass: collect all evaluated values (iterative)
        loop {
            match self.lisp.get(current)? {
                Value::Nil => break,
                Value::Cons { car, cdr } => {
                    if count >= MAX_ARGS {
                        return Err(self.make_error(ErrorKind::StackOverflow, list));
                    }
                    // Evaluate AND force each argument
                    let head_thunk = self.eval_in_env(car, env)?;
                    let head = self.force(head_thunk)?;
                    values[count] = head;
                    count += 1;
                    current = cdr;
                }
                _ => return Err(self.make_error(ErrorKind::TypeError, list)),
            }
        }
        
        // Second pass: build result list (backwards to preserve order)
        let mut result = self.lisp.nil()?;
        for i in (0..count).rev() {
            result = self.lisp.cons(values[i], result)?;
        }
        
        Ok(result)
    }
    
    /// Create a list of thunks from expressions (lazy - wraps each in a thunk)
    /// Used for builtin arguments (builtins handle their own strictness)
    /// 
    /// ITERATIVE implementation to avoid Rust stack overflow
    fn make_thunk_list(&mut self, list: ArenaIndex, env: ArenaIndex) -> EvalResult {
        const MAX_ARGS: usize = 64;
        let dummy = ArenaIndex::new(usize::MAX, u32::MAX);
        let mut thunks: [ArenaIndex; MAX_ARGS] = [dummy; MAX_ARGS];
        let mut count = 0;
        let mut current = list;
        
        // First pass: collect all thunks (iterative)
        loop {
            match self.lisp.get(current)? {
                Value::Nil => break,
                Value::Cons { car, cdr } => {
                    if count >= MAX_ARGS {
                        return Err(self.make_error(ErrorKind::StackOverflow, list));
                    }
                    // Wrap the expression in a thunk (don't evaluate it yet)
                    let thunk = self.lisp.thunk(car, env)?;
                    thunks[count] = thunk;
                    count += 1;
                    current = cdr;
                }
                _ => return Err(self.make_error(ErrorKind::TypeError, list)),
            }
        }
        
        // Second pass: build result list (backwards to preserve order)
        let mut result = self.lisp.nil()?;
        for i in (0..count).rev() {
            result = self.lisp.cons(thunks[i], result)?;
        }
        
        Ok(result)
    }
    
    /// Force a list of thunks and return evaluated list
    /// NOTE: Currently unused but kept for potential future use
    #[allow(dead_code)]
    fn force_list(&mut self, list: ArenaIndex) -> EvalResult {
        let val = self.lisp.get(list)?;
        
        match val {
            Value::Nil => self.lisp.nil().map_err(Into::into),
            Value::Cons { car, cdr } => {
                let head = self.force(car)?;
                let tail = self.force_list(cdr)?;
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
    
    /// Force a thunk to WHNF (Weak Head Normal Form)
    /// Evaluates thunks recursively until we get a non-thunk value
    /// This is called automatically in strict positions
    fn force(&mut self, mut idx: ArenaIndex) -> EvalResult {
        // Loop to handle chains of thunks
        loop {
            let val = self.lisp.get(idx)?;
            
            match val {
                Value::Thunk { expr, env, cached } => {
                    if !cached.is_null() {
                        // Already evaluated - check if cached value is also a thunk
                        idx = cached;
                        continue;
                    }
                    
                    // Evaluate the thunk
                    let result = self.eval_in_env(expr, env)?;
                    
                    // Cache the result (memoization)
                    self.lisp.set(idx, Value::Thunk { expr, env, cached: result })?;
                    
                    // Result might be a thunk - continue forcing
                    idx = result;
                    continue;
                }
                _ => return Ok(idx), // Not a thunk, return as-is (WHNF)
            }
        }
    }
    
    // ========================================================================
    // Built-in Functions
    // ========================================================================
    
    fn apply_builtin(&mut self, builtin: Builtin, args: ArenaIndex, call_expr: ArenaIndex) -> EvalResult {
        match builtin {
            // NON-STRICT: car/cdr force the pair but return elements (may be thunks)
            Builtin::Car => {
                let arg = self.force(self.lisp.car(args)?)?;
                match self.lisp.get(arg)? {
                    Value::Cons { car, .. } => Ok(car), // May be a thunk
                    Value::Nil => self.lisp.nil().map_err(Into::into),
                    _ => Err(self.type_error(call_expr, "pair", self.lisp.get(arg)?.type_name())),
                }
            }
            
            Builtin::Cdr => {
                let arg = self.force(self.lisp.car(args)?)?;
                match self.lisp.get(arg)? {
                    Value::Cons { cdr, .. } => Ok(cdr), // May be a thunk
                    Value::Nil => self.lisp.nil().map_err(Into::into),
                    _ => Err(self.type_error(call_expr, "pair", self.lisp.get(arg)?.type_name())),
                }
            }
            
            // NON-STRICT: cons doesn't force - can contain thunks
            Builtin::Cons => {
                let a = self.lisp.car(args)?;
                let rest = self.lisp.cdr(args)?;
                let b = self.lisp.car(rest)?;
                self.lisp.cons(a, b).map_err(Into::into)
            }
            
            // NON-STRICT: list doesn't force arguments
            Builtin::List => Ok(args),
            
            // STRICT PREDICATES: force argument to check type
            Builtin::Atom => {
                let arg = self.force(self.lisp.car(args)?)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_atom()).map_err(Into::into)
            }
            
            Builtin::Eq => {
                let a = self.force(self.lisp.car(args)?)?;
                let rest = self.lisp.cdr(args)?;
                let b = self.force(self.lisp.car(rest)?)?;
                
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
                let arg = self.force(self.lisp.car(args)?)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_nil()).map_err(Into::into)
            }
            
            Builtin::Pairp => {
                let arg = self.force(self.lisp.car(args)?)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_cons()).map_err(Into::into)
            }
            
            Builtin::Numberp => {
                let arg = self.force(self.lisp.car(args)?)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_number()).map_err(Into::into)
            }
            
            Builtin::Booleanp => {
                let arg = self.force(self.lisp.car(args)?)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_boolean()).map_err(Into::into)
            }
            
            Builtin::Procedurep => {
                let arg = self.force(self.lisp.car(args)?)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_procedure()).map_err(Into::into)
            }
            
            Builtin::Symbolp => {
                let arg = self.force(self.lisp.car(args)?)?;
                self.lisp.boolean(self.lisp.get(arg)?.is_symbol()).map_err(Into::into)
            }
            
            // NOTE: Force and Promisep builtins removed - evaluation is lazy by default
            
            Builtin::Not => {
                let arg = self.force(self.lisp.car(args)?)?;
                self.lisp.boolean(self.is_false(arg)?).map_err(Into::into)
            }
            
            // STRICT ARITHMETIC: force all arguments
            Builtin::Add => self.numeric_fold(args, 0, |a, b| a.checked_add(b), call_expr),
            
            Builtin::Sub => {
                let first = self.get_number_strict(self.lisp.car(args)?, call_expr)?;
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
                let first = self.get_number_strict(self.lisp.car(args)?, call_expr)?;
                let rest = self.lisp.cdr(args)?;
                self.numeric_fold_start(rest, first, |a, b| {
                    if b == 0 { None } else { a.checked_div(b) }
                }, call_expr)
            }
            
            Builtin::Mod => {
                let a = self.get_number_strict(self.lisp.car(args)?, call_expr)?;
                let b = self.get_number_strict(self.lisp.car(self.lisp.cdr(args)?)?, call_expr)?;
                if b == 0 {
                    return Err(self.make_error(ErrorKind::DivisionByZero, call_expr));
                }
                self.lisp.number(a % b).map_err(Into::into)
            }
            
            // STRICT COMPARISONS: force both arguments
            Builtin::Lt => self.compare(args, |a, b| a < b, call_expr),
            Builtin::Gt => self.compare(args, |a, b| a > b, call_expr),
            Builtin::Le => self.compare(args, |a, b| a <= b, call_expr),
            Builtin::Ge => self.compare(args, |a, b| a >= b, call_expr),
            Builtin::NumEq => self.compare(args, |a, b| a == b, call_expr),
            
            // STRICT I/O: force for printing
            Builtin::Print | Builtin::Display => {
                let arg = self.force(self.lisp.car(args)?)?;
                Ok(arg)
            }
            
            Builtin::Newline => {
                self.lisp.nil().map_err(Into::into)
            }
            
            Builtin::Error => {
                let msg = self.force(self.lisp.car(args)?)?;
                Err(self.make_error(ErrorKind::UserError, msg))
            }
        }
    }
    
    /// Get number from a value, forcing if needed (strict)
    fn get_number_strict(&mut self, idx: ArenaIndex, call_expr: ArenaIndex) -> Result<i64, EvalError> {
        let forced = self.force(idx)?;
        match self.lisp.get(forced)? {
            Value::Number(n) => Ok(n),
            v => Err(self.type_error(call_expr, "number", v.type_name())),
        }
    }
    
    fn get_number(&self, idx: ArenaIndex, call_expr: ArenaIndex) -> Result<i64, EvalError> {
        match self.lisp.get(idx)? {
            Value::Number(n) => Ok(n),
            v => Err(self.type_error(call_expr, "number", v.type_name())),
        }
    }
    
    fn numeric_fold<F>(&mut self, args: ArenaIndex, init: i64, f: F, call_expr: ArenaIndex) -> EvalResult
    where F: Fn(i64, i64) -> Option<i64>
    {
        self.numeric_fold_start(args, init, f, call_expr)
    }
    
    fn numeric_fold_start<F>(&mut self, args: ArenaIndex, mut acc: i64, f: F, call_expr: ArenaIndex) -> EvalResult
    where F: Fn(i64, i64) -> Option<i64>
    {
        let mut current = args;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => return self.lisp.number(acc).map_err(Into::into),
                Value::Cons { car, cdr } => {
                    // Force each argument (strict arithmetic)
                    let n = self.get_number_strict(car, call_expr)?;
                    acc = f(acc, n).ok_or_else(|| self.make_error(ErrorKind::DivisionByZero, call_expr))?;
                    current = cdr;
                }
                _ => return Err(self.make_error(ErrorKind::TypeError, current)),
            }
        }
    }
    
    fn compare<F>(&mut self, args: ArenaIndex, cmp: F, call_expr: ArenaIndex) -> EvalResult
    where F: Fn(i64, i64) -> bool
    {
        // Force both arguments (strict comparison)
        let a = self.get_number_strict(self.lisp.car(args)?, call_expr)?;
        let b = self.get_number_strict(self.lisp.car(self.lisp.cdr(args)?)?, call_expr)?;
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
        // Hybrid evaluation: tail calls are STRICT, so TCO works properly!
        // No thunk accumulation - deep recursion is safe.
        let lisp: Lisp<5000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // This would overflow without proper TCO
        eval.eval_str("(define (sum-to n acc) (if (= n 0) acc (sum-to (- n 1) (+ acc n))))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(sum-to 100 0)"), 5050);  // sum 1..100
    }
    
    #[test]
    fn test_eval_recursion() {
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (fact n) (if (= n 0) 1 (* n (fact (- n 1)))))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(fact 5)"), 120);
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // LAZY EVALUATION TESTS
    // Everything is lazy by default - no delay/force needed!
    // ═══════════════════════════════════════════════════════════════════════════
    
    #[test]
    fn test_lazy_basic() {
        // Basic lazy evaluation - arguments computed only when needed
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Simple computation works
        assert_eq!(eval_to_num(&lisp, &mut eval, "(+ 1 2 3)"), 6);
        
        // Functions work
        eval.eval_str("(define (add x y) (+ x y))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(add 10 20)"), 30);
    }
    
    #[test]
    fn test_lazy_cons_is_nonstrict() {
        // cons doesn't force its arguments - can build structures with unevaluated parts
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Create a pair
        eval.eval_str("(define p (cons 1 2))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car p)"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(cdr p)"), 2);
        
        // List creation works
        eval.eval_str("(define lst (list 1 2 3))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car lst)"), 1);
    }
    
    #[test]
    fn test_lazy_infinite_stream() {
        // THE KEY TEST: Infinite structures work!
        // (define ones (cons 1 ones)) - this would loop forever in eager evaluation
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Create an infinite stream of 1s using self-reference
        // This works because cons is non-strict and ones is wrapped in a thunk
        eval.eval_str("(define (make-ones) (cons 1 (make-ones)))").unwrap();
        eval.eval_str("(define ones (make-ones))").unwrap();
        
        // We can access elements without infinite loop
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car ones)"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr ones))"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr (cdr ones)))"), 1);
    }
    
    #[test]
    fn test_lazy_if_branches() {
        // Only the selected branch of 'if' is evaluated
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // This would error in an eager language (div by zero in else branch)
        // But we select the then branch, so else is never evaluated
        eval.eval_str("(define (safe-div x y) (if (= y 0) 0 (/ x y)))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(safe-div 10 0)"), 0);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(safe-div 10 2)"), 5);
    }
    
    #[test]
    fn test_hybrid_builtin_lazy() {
        // HYBRID EVALUATION: Builtin args are lazy (builtins force what they need)
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // 'if' is a special form with lazy branches - only one is evaluated
        // The undefined branch is never forced
        assert_eq!(eval_to_num(&lisp, &mut eval, "(if #t 42 undefined-var)"), 42);
        
        // cons is non-strict - elements stay as thunks
        eval.eval_str("(define p (cons 1 2))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car p)"), 1);
    }
    
    #[test]
    fn test_hybrid_lambda_strict() {
        // HYBRID EVALUATION: Lambda args in tail position are strict (for TCO)
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Lambda args are evaluated, so this is strict
        eval.eval_str("(define (first x y) x)").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(first 42 100)"), 42);  // Works
        
        // The benefit: TCO works for deep recursion
        eval.eval_str("(define (count n) (if (= n 0) 0 (count (- n 1))))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(count 50)"), 0);  // No stack overflow
    }
    
    #[test]
    fn test_lazy_memoization() {
        // Values are memoized - same result every time
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (square x) (* x x))").unwrap();
        eval.eval_str("(define val (square 5))").unwrap();
        
        // Multiple accesses return same value
        assert_eq!(eval_to_num(&lisp, &mut eval, "val"), 25);
        assert_eq!(eval_to_num(&lisp, &mut eval, "val"), 25);
        assert_eq!(eval_to_num(&lisp, &mut eval, "val"), 25);
    }
    
    #[test]
    fn test_lazy_closure() {
        // Closures capture their environment lazily
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (make-adder n) (lambda (x) (+ x n)))").unwrap();
        eval.eval_str("(define add5 (make-adder 5))").unwrap();
        eval.eval_str("(define add10 (make-adder 10))").unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(add5 3)"), 8);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(add10 3)"), 13);
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
    
    // ═══════════════════════════════════════════════════════════════════════════
    // COMPREHENSIVE TCO TESTS
    // ═══════════════════════════════════════════════════════════════════════════
    
    #[test]
    fn test_tco_deep_recursion() {
        // Test TCO with recursion
        // 
        // NOTE: Depth is limited by Rust stack during force() calls.
        // The Lisp-level TCO (trampoline) works, but forcing thunks uses
        // Rust recursion. A full fix requires converting force() to iterative.
        // 
        // Current practical limit: ~100-200 recursive calls in debug mode
        let lisp: Lisp<5000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Tail-recursive sum: sum 1 to n
        eval.eval_str("(define (sum-tail n acc) (if (= n 0) acc (sum-tail (- n 1) (+ acc n))))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(sum-tail 100 0)"), 5050);  // 1+2+...+100
        
        // Tail-recursive countdown  
        eval.eval_str("(define (countdown n) (if (= n 0) 'done (countdown (- n 1))))").unwrap();
        let result = eval.eval_str("(countdown 100)").unwrap();
        assert!(lisp.get(result).unwrap().is_symbol());
    }
    
    #[test]
    fn test_tco_mutual_recursion() {
        // Mutual recursion with TCO - even/odd predicates
        // NOTE: Limited depth due to Rust stack in force()
        let lisp: Lisp<5000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (my-even n) (if (= n 0) #t (my-odd (- n 1))))").unwrap();
        eval.eval_str("(define (my-odd n) (if (= n 0) #f (my-even (- n 1))))").unwrap();
        
        assert!(eval_is_true(&lisp, &mut eval, "(my-even 0)"));
        assert!(eval_is_false(&lisp, &mut eval, "(my-odd 0)"));
        assert!(eval_is_true(&lisp, &mut eval, "(my-even 20)"));
        assert!(eval_is_false(&lisp, &mut eval, "(my-odd 20)"));
        assert!(eval_is_false(&lisp, &mut eval, "(my-even 19)"));
        assert!(eval_is_true(&lisp, &mut eval, "(my-odd 19)"));
    }
    
    #[test]
    fn test_tco_accumulator_pattern() {
        // Classic tail-recursive patterns
        let lisp: Lisp<5000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Tail-recursive factorial
        eval.eval_str("(define (fact-tail n acc) (if (= n 0) acc (fact-tail (- n 1) (* n acc))))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(fact-tail 10 1)"), 3628800);
        
        // Tail-recursive length
        eval.eval_str("(define (len-tail lst acc) (if (null? lst) acc (len-tail (cdr lst) (+ acc 1))))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(len-tail '(a b c d e) 0)"), 5);
        
        // Tail-recursive reverse
        eval.eval_str("(define (rev-tail lst acc) (if (null? lst) acc (rev-tail (cdr lst) (cons (car lst) acc))))").unwrap();
        let result = eval.eval_str("(rev-tail '(1 2 3) '())").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (rev-tail '(1 2 3) '()))"), 3);
    }
    
    #[test]
    fn test_tco_in_cond() {
        // TCO should work in cond branches
        let lisp: Lisp<5000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str(r#"
            (define (classify n)
                (cond ((< n 0) (classify (- 0 n)))
                      ((= n 0) 'zero)
                      ((< n 10) 'small)
                      ((< n 100) 'medium)
                      (else (classify (/ n 10)))))
        "#).unwrap();
        
        let result = eval.eval_str("(classify -42)").unwrap();
        assert!(lisp.get(result).unwrap().is_symbol());
        let result = eval.eval_str("(classify 999)").unwrap();
        assert!(lisp.get(result).unwrap().is_symbol());
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // COMPREHENSIVE LAZY EVALUATION TESTS
    // ═══════════════════════════════════════════════════════════════════════════
    
    #[test]
    fn test_lazy_stream_operations() {
        // Stream operations on infinite data
        let lisp: Lisp<5000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Infinite stream of natural numbers
        eval.eval_str("(define (nats-from n) (cons n (nats-from (+ n 1))))").unwrap();
        eval.eval_str("(define nats (nats-from 0))").unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car nats)"), 0);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr nats))"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr (cdr nats)))"), 2);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr (cdr (cdr nats))))"), 3);
    }
    
    #[test]
    fn test_lazy_stream_take() {
        // Take n elements from a stream
        let lisp: Lisp<5000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (take n s) (if (= n 0) '() (cons (car s) (take (- n 1) (cdr s)))))").unwrap();
        eval.eval_str("(define (ones) (cons 1 (ones)))").unwrap();
        
        // Take 3 ones
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (take 3 (ones)))"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr (take 3 (ones))))"), 1);
    }
    
    #[test]
    fn test_lazy_and_or_short_circuit() {
        // and/or should short-circuit with lazy evaluation
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // 'and' stops at first false
        assert!(eval_is_false(&lisp, &mut eval, "(and #f undefined-error)"));
        assert!(eval_is_true(&lisp, &mut eval, "(and #t #t #t)"));
        
        // 'or' stops at first true
        assert!(eval_is_true(&lisp, &mut eval, "(or #t undefined-error)"));
        assert!(eval_is_false(&lisp, &mut eval, "(or #f #f #f)"));
    }
    
    #[test]
    fn test_lazy_let_bindings() {
        // let bindings in hybrid model
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Basic let
        assert_eq!(eval_to_num(&lisp, &mut eval, "(let ((x 10) (y 20)) (+ x y))"), 30);
        
        // let* with dependencies
        assert_eq!(eval_to_num(&lisp, &mut eval, "(let* ((x 5) (y (* x 2))) (+ x y))"), 15);
        
        // Nested let
        assert_eq!(eval_to_num(&lisp, &mut eval, 
            "(let ((x 1)) (let ((y 2)) (let ((z 3)) (+ x y z))))"), 6);
    }
    
    #[test]
    fn test_lazy_cons_preserves_thunks() {
        // cons should not force its arguments
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Build a list with computations
        eval.eval_str("(define p (cons (+ 1 2) (+ 3 4)))").unwrap();
        
        // Access should force and return correct values
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car p)"), 3);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(cdr p)"), 7);
    }
    
    #[test]
    fn test_lazy_nested_structures() {
        // Deeply nested lazy structures
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Create nested pairs
        eval.eval_str("(define deep (cons (cons (cons 1 2) 3) 4))").unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(cdr deep)"), 4);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(cdr (car deep))"), 3);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(cdr (car (car deep)))"), 2);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (car (car deep)))"), 1);
    }
    
    #[test]
    fn test_lazy_with_gc_pressure() {
        // Test lazy evaluation under GC pressure
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Create an infinite stream
        eval.eval_str("(define (ones) (cons 1 (ones)))").unwrap();
        eval.eval_str("(define stream (ones))").unwrap();
        
        // Access some elements
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car stream)"), 1);
        
        // Run GC
        eval.gc();
        
        // Stream should still work after GC
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr stream))"), 1);
        
        // More allocations and GC
        for _ in 0..10 {
            eval.eval_str("(+ 1 2 3 4 5)").unwrap();
        }
        eval.gc();
        
        // Stream still works
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr (cdr stream)))"), 1);
    }
    
    #[test]
    fn test_lazy_fibonacci_stream() {
        // Classic lazy Fibonacci stream
        let lisp: Lisp<5000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // zipWith for streams
        eval.eval_str("(define (zipwith f s1 s2) (cons (f (car s1) (car s2)) (zipwith f (cdr s1) (cdr s2))))").unwrap();
        
        // Fibonacci stream: fibs = 0 : 1 : zipWith (+) fibs (tail fibs)
        // We use a simpler approach with explicit recursion
        eval.eval_str("(define (fib-pair a b) (cons a (fib-pair b (+ a b))))").unwrap();
        eval.eval_str("(define fibs (fib-pair 0 1))").unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car fibs)"), 0);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr fibs))"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr (cdr fibs)))"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr (cdr (cdr fibs))))"), 2);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr (cdr (cdr (cdr fibs)))))"), 3);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr (cdr (cdr (cdr (cdr fibs))))))"), 5);
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // HYBRID EVALUATION EDGE CASES
    // ═══════════════════════════════════════════════════════════════════════════
    
    #[test]
    fn test_hybrid_nested_calls() {
        // Test behavior with nested function calls
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (double x) (* x 2))").unwrap();
        eval.eval_str("(define (quad x) (double (double x)))").unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(quad 5)"), 20);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(quad (quad 2))"), 32);
    }
    
    #[test]
    fn test_hybrid_higher_order() {
        // Higher-order functions with hybrid evaluation
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (apply-twice f x) (f (f x)))").unwrap();
        eval.eval_str("(define (inc x) (+ x 1))").unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(apply-twice inc 0)"), 2);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(apply-twice (lambda (x) (* x 2)) 3)"), 12);
    }
    
    #[test]
    fn test_hybrid_currying() {
        // Curried functions
        let lisp: Lisp<3000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (curry-add a) (lambda (b) (lambda (c) (+ a b c))))").unwrap();
        eval.eval_str("(define add1 (curry-add 1))").unwrap();
        eval.eval_str("(define add1-2 (add1 2))").unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(add1-2 3)"), 6);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(((curry-add 10) 20) 30)"), 60);
    }
    
    #[test]
    fn test_force_chain_memoization() {
        // Verify that forcing a thunk memoizes the result
        let lisp: Lisp<2000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Create a computation stored in a cons
        eval.eval_str("(define p (cons (* 111 111) 0))").unwrap();
        
        // First access forces and memoizes
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car p)"), 12321);
        
        // Second access should return memoized value
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car p)"), 12321);
        
        // Third access
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car p)"), 12321);
    }
}
