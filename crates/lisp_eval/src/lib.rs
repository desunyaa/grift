#![no_std]

//! # Lisp Evaluator
//!
//! A classic Lisp evaluator with lexically scoped closures.
//!
//! ## Environment
//!
//! Environments are association lists: `((name . value) (name . value) ...)`
//!
//! ## Special Forms
//!
//! - `quote` - Return expression unevaluated
//! - `if` - Conditional
//! - `cond` - Multi-way conditional
//! - `lambda` - Create closure
//! - `define` - Define variable/function
//! - `let` - Local binding
//! - `begin` - Sequence of expressions
//! - `set!` - Mutation

pub use lisp_parser::{
    Arena, ArenaIndex, ArenaError, ArenaResult, Trace, GcStats,
    Value, Builtin, Lisp, ParseError, parse,
};

/// Evaluation error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalError {
    /// Arena is full
    OutOfMemory,
    /// Unbound variable
    UnboundVariable,
    /// Not a function
    NotAFunction,
    /// Wrong number of arguments
    WrongArgCount,
    /// Type error (e.g., car of non-list)
    TypeError,
    /// Division by zero
    DivisionByZero,
    /// Parse error
    ParseError(ParseError),
    /// Generic error
    Error,
}

impl From<ArenaError> for EvalError {
    fn from(e: ArenaError) -> Self {
        match e {
            ArenaError::OutOfMemory => EvalError::OutOfMemory,
            _ => EvalError::Error,
        }
    }
}

impl From<ParseError> for EvalError {
    fn from(e: ParseError) -> Self {
        EvalError::ParseError(e)
    }
}

/// Result type for evaluation
pub type EvalResult = Result<ArenaIndex, EvalError>;

/// The Lisp evaluator
pub struct Evaluator<'a, const N: usize> {
    lisp: &'a Lisp<N>,
    /// Global environment
    global_env: ArenaIndex,
    /// Roots for GC (global env + any temps)
    gc_roots: [ArenaIndex; 32],
    gc_root_count: usize,
}

impl<'a, const N: usize> Evaluator<'a, N> {
    /// Create a new evaluator with standard environment
    pub fn new(lisp: &'a Lisp<N>) -> Result<Self, EvalError> {
        let mut eval = Evaluator {
            lisp,
            global_env: ArenaIndex::NULL,
            gc_roots: [ArenaIndex::NULL; 32],
            gc_root_count: 0,
        };
        
        // Initialize global environment with builtins
        eval.global_env = lisp.nil()?;
        
        for &builtin in Builtin::ALL {
            let name = lisp.symbol(builtin.name())?;
            let val = lisp.builtin(builtin)?;
            eval.global_env = eval.env_extend(eval.global_env, name, val)?;
        }
        
        // Add 't' as true
        let t = lisp.symbol("t")?;
        eval.global_env = eval.env_extend(eval.global_env, t, t)?;
        
        Ok(eval)
    }
    
    /// Get the Lisp context
    pub fn lisp(&self) -> &Lisp<N> {
        self.lisp
    }
    
    /// Get the global environment root
    pub fn global_env(&self) -> ArenaIndex {
        self.global_env
    }
    
    /// Run GC with current roots
    pub fn gc(&self) -> GcStats {
        let mut roots = [ArenaIndex::NULL; 33];
        roots[0] = self.global_env;
        for i in 0..self.gc_root_count {
            roots[i + 1] = self.gc_roots[i];
        }
        self.lisp.gc(&roots[..self.gc_root_count + 1])
    }
    
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
                Value::Nil => return Err(EvalError::UnboundVariable),
                Value::Cons { car, cdr } => {
                    // car is (name . value)
                    let binding = self.lisp.get(car)?;
                    if let Value::Cons { car: bound_name, cdr: bound_value } = binding {
                        if self.lisp.symbol_eq(bound_name, name)? {
                            return Ok(bound_value);
                        }
                    }
                    current = cdr;
                }
                _ => return Err(EvalError::Error),
            }
        }
    }
    
    /// Set a variable in the environment (mutation)
    fn env_set(&self, env: ArenaIndex, name: ArenaIndex, value: ArenaIndex) -> EvalResult {
        let mut current = env;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => return Err(EvalError::UnboundVariable),
                Value::Cons { car, cdr } => {
                    let binding = self.lisp.get(car)?;
                    if let Value::Cons { car: bound_name, cdr: _ } = binding {
                        if self.lisp.symbol_eq(bound_name, name)? {
                            // Update the binding in place
                            self.lisp.set(car, Value::Cons { car: bound_name, cdr: value })?;
                            return Ok(value);
                        }
                    }
                    current = cdr;
                }
                _ => return Err(EvalError::Error),
            }
        }
    }
    
    /// Define in global environment
    pub fn define(&mut self, name: ArenaIndex, value: ArenaIndex) -> EvalResult {
        // Check if already defined and update, otherwise add
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
                _ => return Err(EvalError::Error),
            }
        }
    }
    
    /// Evaluate an expression
    pub fn eval(&mut self, expr: ArenaIndex) -> EvalResult {
        self.eval_in_env(expr, self.global_env)
    }
    
    /// Evaluate an expression in a given environment
    fn eval_in_env(&mut self, expr: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let val = self.lisp.get(expr)?;
        
        match val {
            // Self-evaluating
            Value::Nil | Value::Number(_) | Value::Char(_) | 
            Value::Builtin(_) | Value::Lambda { .. } => Ok(expr),
            
            // Symbol - variable lookup
            Value::Symbol { .. } => {
                // Try local env first, then global
                match self.env_lookup(env, expr) {
                    Ok(v) => Ok(v),
                    Err(EvalError::UnboundVariable) => self.env_lookup(self.global_env, expr),
                    Err(e) => Err(e),
                }
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
                    
                    // if
                    if self.lisp.symbol_matches(car, "if")? {
                        return self.eval_if(cdr, env);
                    }
                    
                    // cond
                    if self.lisp.symbol_matches(car, "cond")? {
                        return self.eval_cond(cdr, env);
                    }
                    
                    // lambda
                    if self.lisp.symbol_matches(car, "lambda")? {
                        return self.eval_lambda(cdr, env);
                    }
                    
                    // define
                    if self.lisp.symbol_matches(car, "define")? {
                        return self.eval_define(cdr, env);
                    }
                    
                    // let
                    if self.lisp.symbol_matches(car, "let")? {
                        return self.eval_let(cdr, env);
                    }
                    
                    // begin
                    if self.lisp.symbol_matches(car, "begin")? {
                        return self.eval_begin(cdr, env);
                    }
                    
                    // set!
                    if self.lisp.symbol_matches(car, "set!")? {
                        return self.eval_set(cdr, env);
                    }
                    
                    // and
                    if self.lisp.symbol_matches(car, "and")? {
                        return self.eval_and(cdr, env);
                    }
                    
                    // or
                    if self.lisp.symbol_matches(car, "or")? {
                        return self.eval_or(cdr, env);
                    }
                }
                
                // Function application
                let func = self.eval_in_env(car, env)?;
                let args = self.eval_list(cdr, env)?;
                self.apply(func, args)
            }
        }
    }
    
    /// Evaluate a list of expressions
    fn eval_list(&mut self, list: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let val = self.lisp.get(list)?;
        
        match val {
            Value::Nil => self.lisp.nil().map_err(Into::into),
            Value::Cons { car, cdr } => {
                let head = self.eval_in_env(car, env)?;
                let tail = self.eval_list(cdr, env)?;
                self.lisp.cons(head, tail).map_err(Into::into)
            }
            _ => Err(EvalError::TypeError),
        }
    }
    
    /// Evaluate if special form
    fn eval_if(&mut self, args: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let cond = self.lisp.car(args)?;
        let rest = self.lisp.cdr(args)?;
        let then_branch = self.lisp.car(rest)?;
        let else_rest = self.lisp.cdr(rest)?;
        
        let cond_val = self.eval_in_env(cond, env)?;
        
        if !self.is_falsy(cond_val)? {
            self.eval_in_env(then_branch, env)
        } else {
            // else branch is optional
            match self.lisp.get(else_rest)? {
                Value::Nil => self.lisp.nil().map_err(Into::into),
                Value::Cons { car, .. } => self.eval_in_env(car, env),
                _ => Err(EvalError::TypeError),
            }
        }
    }
    
    /// Evaluate cond special form
    fn eval_cond(&mut self, clauses: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let mut current = clauses;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => return self.lisp.nil().map_err(Into::into),
                Value::Cons { car: clause, cdr: rest } => {
                    let test = self.lisp.car(clause)?;
                    let body = self.lisp.cdr(clause)?;
                    
                    // Check for 'else' clause
                    let is_else = self.lisp.symbol_matches(test, "else").unwrap_or(false);
                    
                    let test_val = if is_else {
                        self.lisp.symbol("t")? // Always true
                    } else {
                        self.eval_in_env(test, env)?
                    };
                    
                    if !self.is_falsy(test_val)? {
                        // Evaluate body expressions
                        return self.eval_begin(body, env);
                    }
                    
                    current = rest;
                }
                _ => return Err(EvalError::TypeError),
            }
        }
    }
    
    /// Evaluate lambda special form
    fn eval_lambda(&mut self, args: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let params = self.lisp.car(args)?;
        let body_list = self.lisp.cdr(args)?;
        
        // Wrap body in begin if multiple expressions
        let body = self.lisp.car(body_list)?;
        let rest = self.lisp.cdr(body_list)?;
        
        let body = if self.lisp.get(rest)?.is_nil() {
            body
        } else {
            // Multiple body expressions - wrap in begin
            let begin = self.lisp.symbol("begin")?;
            self.lisp.cons(begin, body_list)?
        };
        
        self.lisp.lambda(params, body, env).map_err(Into::into)
    }
    
    /// Evaluate define special form
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
                let lambda_sym = self.lisp.symbol("lambda")?;
                let lambda_body = self.lisp.cons(params, rest)?;
                let lambda_expr = self.lisp.cons(lambda_sym, lambda_body)?;
                let lambda = self.eval_in_env(lambda_expr, env)?;
                self.define(name, lambda)
            }
            _ => Err(EvalError::TypeError),
        }
    }
    
    /// Evaluate let special form
    fn eval_let(&mut self, args: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let bindings = self.lisp.car(args)?;
        let body = self.lisp.cdr(args)?;
        
        // Extend environment with bindings
        let mut new_env = env;
        let mut current = bindings;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => break,
                Value::Cons { car: binding, cdr: rest } => {
                    let name = self.lisp.car(binding)?;
                    let value_expr = self.lisp.car(self.lisp.cdr(binding)?)?;
                    let value = self.eval_in_env(value_expr, env)?;
                    new_env = self.env_extend(new_env, name, value)?;
                    current = rest;
                }
                _ => return Err(EvalError::TypeError),
            }
        }
        
        // Evaluate body in new environment
        self.eval_begin(body, new_env)
    }
    
    /// Evaluate begin special form (sequence)
    fn eval_begin(&mut self, exprs: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let mut current = exprs;
        let mut result = self.lisp.nil()?;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => return Ok(result),
                Value::Cons { car: expr, cdr: rest } => {
                    result = self.eval_in_env(expr, env)?;
                    current = rest;
                }
                _ => return Err(EvalError::TypeError),
            }
        }
    }
    
    /// Evaluate set! special form
    fn eval_set(&mut self, args: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let name = self.lisp.car(args)?;
        let value_expr = self.lisp.car(self.lisp.cdr(args)?)?;
        let value = self.eval_in_env(value_expr, env)?;
        
        // Try local env first, then global
        match self.env_set(env, name, value) {
            Ok(v) => Ok(v),
            Err(EvalError::UnboundVariable) => {
                // Try global
                match self.env_set(self.global_env, name, value) {
                    Ok(v) => Ok(v),
                    Err(_) => Err(EvalError::UnboundVariable),
                }
            }
            Err(e) => Err(e),
        }
    }
    
    /// Evaluate and special form
    fn eval_and(&mut self, args: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let mut current = args;
        let mut result = self.lisp.symbol("t")?; // Default true
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => return Ok(result),
                Value::Cons { car: expr, cdr: rest } => {
                    result = self.eval_in_env(expr, env)?;
                    if self.is_falsy(result)? {
                        return self.lisp.nil().map_err(Into::into);
                    }
                    current = rest;
                }
                _ => return Err(EvalError::TypeError),
            }
        }
    }
    
    /// Evaluate or special form
    fn eval_or(&mut self, args: ArenaIndex, env: ArenaIndex) -> EvalResult {
        let mut current = args;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => return self.lisp.nil().map_err(Into::into),
                Value::Cons { car: expr, cdr: rest } => {
                    let result = self.eval_in_env(expr, env)?;
                    if !self.is_falsy(result)? {
                        return Ok(result);
                    }
                    current = rest;
                }
                _ => return Err(EvalError::TypeError),
            }
        }
    }
    
    /// Check if a value is falsy (nil)
    fn is_falsy(&self, val: ArenaIndex) -> Result<bool, EvalError> {
        Ok(self.lisp.get(val)?.is_nil())
    }
    
    /// Apply a function to arguments
    fn apply(&mut self, func: ArenaIndex, args: ArenaIndex) -> EvalResult {
        let func_val = self.lisp.get(func)?;
        
        match func_val {
            Value::Builtin(b) => self.apply_builtin(b, args),
            Value::Lambda { params, body, env } => {
                // Extend environment with argument bindings
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
                        (Value::Nil, _) => return Err(EvalError::WrongArgCount),
                        (_, Value::Nil) => return Err(EvalError::WrongArgCount),
                        _ => return Err(EvalError::TypeError),
                    }
                }
                
                self.eval_in_env(body, new_env)
            }
            _ => Err(EvalError::NotAFunction),
        }
    }
    
    /// Apply a builtin function
    fn apply_builtin(&mut self, builtin: Builtin, args: ArenaIndex) -> EvalResult {
        match builtin {
            Builtin::Car => {
                let arg = self.lisp.car(args)?;
                Ok(self.lisp.car(arg)?)
            }
            Builtin::Cdr => {
                let arg = self.lisp.car(args)?;
                Ok(self.lisp.cdr(arg)?)
            }
            Builtin::Cons => {
                let a = self.lisp.car(args)?;
                let rest = self.lisp.cdr(args)?;
                let b = self.lisp.car(rest)?;
                self.lisp.cons(a, b).map_err(Into::into)
            }
            Builtin::List => Ok(args), // Already a list!
            Builtin::Atom => {
                let arg = self.lisp.car(args)?;
                let val = self.lisp.get(arg)?;
                if val.is_atom() {
                    self.lisp.symbol("t").map_err(Into::into)
                } else {
                    self.lisp.nil().map_err(Into::into)
                }
            }
            Builtin::Eq => {
                let a = self.lisp.car(args)?;
                let rest = self.lisp.cdr(args)?;
                let b = self.lisp.car(rest)?;
                
                let val_a = self.lisp.get(a)?;
                let val_b = self.lisp.get(b)?;
                
                let eq = match (val_a, val_b) {
                    (Value::Nil, Value::Nil) => true,
                    (Value::Number(x), Value::Number(y)) => x == y,
                    (Value::Char(x), Value::Char(y)) => x == y,
                    (Value::Symbol { .. }, Value::Symbol { .. }) => {
                        self.lisp.symbol_eq(a, b)?
                    }
                    _ => a == b, // Same index
                };
                
                if eq {
                    self.lisp.symbol("t").map_err(Into::into)
                } else {
                    self.lisp.nil().map_err(Into::into)
                }
            }
            Builtin::Null => {
                let arg = self.lisp.car(args)?;
                if self.lisp.get(arg)?.is_nil() {
                    self.lisp.symbol("t").map_err(Into::into)
                } else {
                    self.lisp.nil().map_err(Into::into)
                }
            }
            Builtin::Numberp => {
                let arg = self.lisp.car(args)?;
                if self.lisp.get(arg)?.is_number() {
                    self.lisp.symbol("t").map_err(Into::into)
                } else {
                    self.lisp.nil().map_err(Into::into)
                }
            }
            Builtin::Add => self.numeric_fold(args, 0, |a, b| a.checked_add(b)),
            Builtin::Sub => {
                let first = self.get_number(self.lisp.car(args)?)?;
                let rest = self.lisp.cdr(args)?;
                if self.lisp.get(rest)?.is_nil() {
                    // Unary minus
                    self.lisp.number(-first).map_err(Into::into)
                } else {
                    // Subtraction
                    let result = self.numeric_fold_start(rest, first, |a, b| a.checked_sub(b))?;
                    Ok(result)
                }
            }
            Builtin::Mul => self.numeric_fold(args, 1, |a, b| a.checked_mul(b)),
            Builtin::Div => {
                let first = self.get_number(self.lisp.car(args)?)?;
                let rest = self.lisp.cdr(args)?;
                self.numeric_fold_start(rest, first, |a, b| {
                    if b == 0 { None } else { a.checked_div(b) }
                })
            }
            Builtin::Mod => {
                let a = self.get_number(self.lisp.car(args)?)?;
                let b = self.get_number(self.lisp.car(self.lisp.cdr(args)?)?)?;
                if b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                self.lisp.number(a % b).map_err(Into::into)
            }
            Builtin::Lt => self.compare(args, |a, b| a < b),
            Builtin::Gt => self.compare(args, |a, b| a > b),
            Builtin::Le => self.compare(args, |a, b| a <= b),
            Builtin::Ge => self.compare(args, |a, b| a >= b),
            Builtin::NumEq => self.compare(args, |a, b| a == b),
            Builtin::Print => {
                // Print is a no-op in no_std eval, handled by REPL
                Ok(self.lisp.car(args)?)
            }
            Builtin::Newline => {
                // No-op in no_std
                self.lisp.nil().map_err(Into::into)
            }
        }
    }
    
    /// Get number from value
    fn get_number(&self, idx: ArenaIndex) -> Result<i64, EvalError> {
        match self.lisp.get(idx)? {
            Value::Number(n) => Ok(n),
            _ => Err(EvalError::TypeError),
        }
    }
    
    /// Fold over numeric arguments
    fn numeric_fold<F>(&self, args: ArenaIndex, init: i64, f: F) -> EvalResult
    where
        F: Fn(i64, i64) -> Option<i64>,
    {
        self.numeric_fold_start(args, init, f)
    }
    
    fn numeric_fold_start<F>(&self, args: ArenaIndex, mut acc: i64, f: F) -> EvalResult
    where
        F: Fn(i64, i64) -> Option<i64>,
    {
        let mut current = args;
        
        loop {
            match self.lisp.get(current)? {
                Value::Nil => return self.lisp.number(acc).map_err(Into::into),
                Value::Cons { car, cdr } => {
                    let n = self.get_number(car)?;
                    acc = f(acc, n).ok_or(EvalError::DivisionByZero)?;
                    current = cdr;
                }
                _ => return Err(EvalError::TypeError),
            }
        }
    }
    
    /// Compare two numbers
    fn compare<F>(&self, args: ArenaIndex, cmp: F) -> EvalResult
    where
        F: Fn(i64, i64) -> bool,
    {
        let a = self.get_number(self.lisp.car(args)?)?;
        let b = self.get_number(self.lisp.car(self.lisp.cdr(args)?)?)?;
        
        if cmp(a, b) {
            self.lisp.symbol("t").map_err(Into::into)
        } else {
            self.lisp.nil().map_err(Into::into)
        }
    }
    
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
    
    fn eval_to_num(lisp: &Lisp<1000>, eval: &mut Evaluator<1000>, input: &str) -> i64 {
        let result = eval.eval_str(input).unwrap();
        lisp.get(result).unwrap().as_number().unwrap()
    }
    
    fn eval_is_nil(lisp: &Lisp<1000>, eval: &mut Evaluator<1000>, input: &str) -> bool {
        let result = eval.eval_str(input).unwrap();
        lisp.get(result).unwrap().is_nil()
    }
    
    #[test]
    fn test_eval_number() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "42"), 42);
        assert_eq!(eval_to_num(&lisp, &mut eval, "-10"), -10);
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
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(if t 1 2)"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(if nil 1 2)"), 2);
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
    fn test_eval_recursion() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        eval.eval_str("(define (fact n) (if (= n 0) 1 (* n (fact (- n 1)))))").unwrap();
        assert_eq!(eval_to_num(&lisp, &mut eval, "(fact 5)"), 120);
    }
    
    #[test]
    fn test_eval_cons_car_cdr() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car '(1 2 3))"), 1);
        assert_eq!(eval_to_num(&lisp, &mut eval, "(car (cdr '(1 2 3)))"), 2);
    }
    
    fn eval_is_truthy(lisp: &Lisp<1000>, eval: &mut Evaluator<1000>, input: &str) -> bool {
        let result = eval.eval_str(input).unwrap();
        !lisp.get(result).unwrap().is_nil()
    }
    
    #[test]
    fn test_eval_list_functions() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // (null '()) should return t (truthy)
        assert!(eval_is_truthy(&lisp, &mut eval, "(null '())"));
        assert!(eval_is_truthy(&lisp, &mut eval, "(null nil)"));
        // (null '(1)) should return nil (falsy)
        assert!(!eval_is_truthy(&lisp, &mut eval, "(null '(1))"));
    }
    
    #[test]
    fn test_eval_cond() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        let result = eval_to_num(&lisp, &mut eval, 
            "(cond ((< 5 3) 1) ((> 5 3) 2) (else 3))");
        assert_eq!(result, 2);
    }
    
    #[test]
    fn test_gc_during_eval() {
        let lisp: Lisp<1000> = Lisp::new();
        let mut eval = Evaluator::new(&lisp).unwrap();
        
        // Define something
        eval.eval_str("(define x 42)").unwrap();
        
        // Run GC
        let _stats = eval.gc();
        
        // Value should still be accessible
        assert_eq!(eval_to_num(&lisp, &mut eval, "x"), 42);
    }
}
