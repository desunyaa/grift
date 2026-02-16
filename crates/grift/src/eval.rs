//! Lisp evaluator with call-by-need (lazy) semantics.
//!
//! Evaluates arena-allocated S-expressions in an environment using
//! call-by-need evaluation with memoization and tail-call optimization.
//! Lazy values use a poll-based protocol inspired by `core::future::Future`:
//! `Lazy` → `Polling` → `Ready`.

use grift_arena::{ArenaError, ArenaIndex, ArenaResult};

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use crate::lisp::Lisp;
use crate::value::{BuiltinId, Value};

// ── No-op Waker infrastructure and minimal single-threaded executor ──────

/// No-op clone function for the [`RawWakerVTable`].
fn noop_clone(_: *const ()) -> RawWaker {
    noop_raw_waker()
}

/// No-op function for wake, wake_by_ref, and drop in the [`RawWakerVTable`].
fn noop(_: *const ()) {}

/// VTable for the no-op [`RawWaker`]. All operations are no-ops because
/// our synchronous executor never needs real wake notifications.
const NOOP_VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);

/// Build a no-op [`RawWaker`] using our [`NOOP_VTABLE`].
///
/// This is the low-level building block that could construct a [`Waker`]
/// via `unsafe { Waker::from_raw(noop_raw_waker()) }`.  Since we use
/// `Waker::noop()` (safe, no-std) for actual waker creation, this
/// function serves as documentation of the raw waker infrastructure.
fn noop_raw_waker() -> RawWaker {
    RawWaker::new(core::ptr::null(), &NOOP_VTABLE)
}

/// Minimal single-threaded executor: repeatedly polls a future to
/// completion.  This is the top-level driver for async evaluation.
///
/// Uses the no-op [`Waker`] provided by `Waker::noop()` because the
/// evaluator is fully synchronous — `Poll::Pending` is only used as a
/// signal to re-enter the eval loop (e.g., for tail-call optimization),
/// not for actual I/O readiness.
///
/// The raw waker infrastructure ([`noop_raw_waker`], [`NOOP_VTABLE`])
/// is available for platforms where `Waker::noop()` is not supported.
fn block_on<F: Future + Unpin>(mut future: F) -> F::Output {
    // Ensure the RawWaker/RawWakerVTable infrastructure is referenced.
    let _raw_waker: fn() -> RawWaker = noop_raw_waker;
    let waker: &Waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    loop {
        match Pin::new(&mut future).poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => continue,
        }
    }
}

/// A stack-local wrapper that implements [`Future`] for forcing a lazy
/// arena value to Weak Head Normal Form.
///
/// This is a temporary object that lives on the stack during forcing —
/// the lazy/polling/ready state itself lives in the arena as `Value`
/// variants.
struct ForceFuture<'a, 'b, const N: usize> {
    evaluator: &'b mut Evaluator<'a, N>,
    idx: ArenaIndex,
}

impl<'a, 'b, const N: usize> Future for ForceFuture<'a, 'b, N> {
    type Output = ArenaResult<ArenaIndex>;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let idx = self.idx;
        let val = match self.evaluator.lisp.get(idx) {
            Ok(v) => v,
            Err(e) => return Poll::Ready(Err(e)),
        };

        if val.is_whnf() {
            return Poll::Ready(Ok(idx));
        }

        match val {
            // Ready — follow the memoized result pointer, re-poll.
            Value::Ready(target) => {
                self.idx = target;
                Poll::Pending
            }

            // Lazy — begin polling: set Polling sentinel, evaluate, memoize.
            Value::Lazy { expr, env } => {
                let lazy_idx = idx;
                if let Err(e) = self.evaluator.lisp.arena.set(lazy_idx, Value::Polling) {
                    return Poll::Ready(Err(e));
                }
                self.evaluator.push_root(lazy_idx);

                let result = match self.evaluator.eval(expr, env) {
                    Ok(r) => r,
                    Err(e) => {
                        self.evaluator.pop_roots(1);
                        return Poll::Ready(Err(e));
                    }
                };
                self.evaluator.pop_roots(1);

                // Follow ready chain, detecting cycles via polling sentinel.
                let mut target = result;
                loop {
                    match self.evaluator.lisp.get(target) {
                        Ok(Value::Ready(t)) => target = t,
                        Ok(Value::Polling) => {
                            return Poll::Ready(Err(ArenaError::BlackHoleDetected));
                        }
                        Err(e) => return Poll::Ready(Err(e)),
                        _ => break,
                    }
                }

                // Memoize: overwrite lazy cell with ready.
                if let Err(e) = self.evaluator.lisp.arena.set(lazy_idx, Value::Ready(target)) {
                    return Poll::Ready(Err(e));
                }
                self.idx = target;
                Poll::Pending
            }

            // Circular dependency detected.
            Value::Polling => Poll::Ready(Err(ArenaError::BlackHoleDetected)),

            _ => Poll::Ready(Err(ArenaError::TypeError)),
        }
    }
}

// ForceFuture is Unpin because it only contains a mutable reference and an ArenaIndex,
// neither of which are self-referential.
impl<'a, 'b, const N: usize> Unpin for ForceFuture<'a, 'b, N> {}

/// Convert a fallible closure into a `TailAction`: `Ok(())` → `Continue`,
/// `Err(e)` → `Return(Err(e))`.  Eliminates the repeated match boilerplate
/// in every TCO-aware special form.
macro_rules! tail_continue {
    ($body:expr) => {
        match $body {
            Ok(()) => TailAction::Continue,
            Err(e) => TailAction::Return(Err(e)),
        }
    };
}

/// Generate a type-predicate builtin method that checks the first arg
/// against a pattern.  All six predicates (`null?`, `pair?`, `number?`, …)
/// share the exact same shape; this macro captures it once.
macro_rules! type_predicate {
    ($name:ident, $pat:pat) => {
        fn $name(&self, args: ArenaIndex) -> ArenaResult<ArenaIndex> {
            let val = self.lisp.car(args)?;
            self.lisp.boolean(matches!(self.lisp.get(val)?, $pat))
        }
    };
}

/// Fold a variadic argument list over a checked arithmetic operation,
/// starting from `$init`.  Used by `(+ ...)` and `(* ...)`.
macro_rules! fold_numbers {
    ($self:ident, $args:ident, $init:expr, $op:ident) => {{
        let mut acc: isize = $init;
        let mut cur = $args;
        while !cur.is_nil() {
            let n = $self.lisp.get($self.lisp.car(cur)?)?.as_number()?;
            acc = acc.$op(n).ok_or(ArenaError::ArithmeticOverflow)?;
            cur = $self.lisp.cdr(cur)?;
        }
        $self.lisp.number(acc)
    }};
}

/// Generate a numeric comparison builtin method.
macro_rules! cmp_builtin {
    ($name:ident, $op:tt) => {
        fn $name(&self, args: ArenaIndex) -> ArenaResult<ArenaIndex> {
            let (a, b) = binary_nums!(self, args);
            self.lisp.boolean(a $op b)
        }
    };
}
macro_rules! binary_nums {
    ($self:ident, $args:ident) => {{
        let a = $self.lisp.get($self.lisp.car($args)?)?.as_number()?;
        let b = $self
            .lisp
            .get($self.lisp.car($self.lisp.cdr($args)?)?)?
            .as_number()?;
        (a, b)
    }};
}

/// Generate a pair-accessor builtin (`car` or `cdr`) that extracts a
/// component from the first argument and forces the result.
macro_rules! pair_builtin {
    ($name:ident, $accessor:ident) => {
        fn $name(&mut self, args: ArenaIndex) -> ArenaResult<ArenaIndex> {
            let pair = self.lisp.car(args)?;
            self.force_async(self.lisp.$accessor(pair)?)
        }
    };
}

/// Non-tail special form: wrap a `(args, env) -> Result` method as a
/// TCO `TailAction::Return`.
#[inline]
fn non_tail(result: ArenaResult<ArenaIndex>) -> TailAction {
    TailAction::Return(result)
}
///
/// **Built-in functions** (section `builtins { ... }`) are registered in the
/// global environment as `Value::Builtin(id)` with auto-assigned sequential
/// IDs.  Their arguments are evaluated and forced to WHNF before the handler
/// is called (strict evaluation).
///
/// **Special forms** (section `special_forms { ... }`) are matched by symbol
/// name during evaluation and receive their arguments *unevaluated*.
/// Some special forms (e.g., `cons`) implement lazy semantics by wrapping
/// arguments in thunks.
macro_rules! define_builtins {
    (
        builtins {
            $( $bname:literal => $bmethod:ident ),* $(,)?
        }
        special_forms {
            $( $sname:literal => $smethod:ident ),* $(,)?
        }
    ) => {
        // Generate unique constant IDs for each builtin.
        define_builtins!(@consts 0u8, $( $bname, $bmethod; )* );

        impl<'a, const N: usize> Evaluator<'a, N> {
            /// Register all built-in functions in the global environment.
            fn init_builtins(&mut self) {
                $(
                    if let (Ok(sym), Ok(val)) = (
                        self.lisp.symbol($bname),
                        self.lisp.arena.alloc(Value::Builtin(
                            define_builtins!(@const_name $bmethod)
                        )),
                    ) {
                        if let Ok(new_env) = env_bind(self.lisp, self.global_env, sym, val) {
                            self.global_env = new_env;
                        }
                    }
                )*
            }

            /// Apply a built-in function.
            #[allow(non_upper_case_globals)]
            fn apply_builtin(&mut self, id: BuiltinId, args: ArenaIndex) -> ArenaResult<ArenaIndex> {
                match id {
                    $( define_builtins!(@const_name $bmethod) => self.$bmethod(args), )*
                    _ => Err(ArenaError::NotCallable),
                }
            }

            /// Try to dispatch a special form by symbol name (TCO-aware).
            ///
            /// Returns `Some(TailAction)` if `car` matched a special form,
            /// `None` otherwise (fall through to function application).
            fn try_special_form_tco(
                &mut self,
                car: ArenaIndex,
                cdr: ArenaIndex,
                expr: &mut ArenaIndex,
                env: &mut ArenaIndex,
            ) -> Option<TailAction> {
                $(
                    if self.lisp.symbol_name_eq(car, $sname) {
                        return Some(self.$smethod(cdr, expr, env));
                    }
                )*
                None
            }
        }
    };

    // Generate const declarations recursively with incrementing IDs.
    (@consts $id:expr, $name:literal, $method:ident; $( $rest_name:literal, $rest_method:ident; )* ) => {
        define_builtins!(@make_const $method, $id);
        define_builtins!(@consts $id + 1u8, $( $rest_name, $rest_method; )* );
    };
    (@consts $id:expr, ) => {};

    // Generate a single const with a name derived from the method name.
    (@make_const $method:ident, $id:expr) => {
        #[allow(non_upper_case_globals)]
        const $method: BuiltinId = BuiltinId($id);
    };

    // Reference a const by method name.
    (@const_name $method:ident) => { $method };
}

// Invoke the macro to generate `init_builtins`, `apply_builtin`,
// and `try_special_form_tco`.
define_builtins! {
    builtins {
        "+"        => builtin_add,
        "-"        => builtin_sub,
        "*"        => builtin_mul,
        "/"        => builtin_div,
        "="        => builtin_eq,
        "<"        => builtin_lt,
        ">"        => builtin_gt,
        "<="       => builtin_le,
        ">="       => builtin_ge,
        "car"      => builtin_car,
        "cdr"      => builtin_cdr,
        "list"     => builtin_list,
        "null?"    => builtin_nullp,
        "not"      => builtin_not,
        "pair?"    => builtin_pairp,
        "number?"  => builtin_numberp,
        "symbol?"  => builtin_symbolp,
        "boolean?" => builtin_booleanp,
    }
    special_forms {
        "quote"  => eval_quote,
        "if"     => eval_if,
        "define" => eval_define,
        "set!"   => eval_set,
        "lambda" => eval_lambda,
        "begin"  => eval_begin,
        "cond"   => eval_cond,
        "and"    => eval_and,
        "or"     => eval_or,
        "let"    => eval_let,
        "cons"   => eval_cons,
    }
}

/// TCO control flow for special forms.
enum TailAction {
    /// Return this value immediately (non-tail position result).
    Return(ArenaResult<ArenaIndex>),
    /// expr and env have been updated; re-enter the eval loop.
    Continue,
}

/// The evaluator state.
pub(crate) struct Evaluator<'a, const N: usize> {
    lisp: &'a Lisp<N>,
    pub global_env: ArenaIndex,
    /// Shadow stack of GC roots stored as a linked list of cons cells in
    /// the arena.  Each entry is `(root_value . rest)`, with `ArenaIndex::NIL`
    /// as the empty list.  This replaces the former fixed-size array, so all
    /// allocation lives inside the arena.
    gc_roots: ArenaIndex,
}

/// Walk an environment association list, calling `f` on each (key, binding)
/// pair until `f` returns `Some(R)`.  Returns `Err(UnboundVariable)` when the
/// name is not found.
#[inline]
fn env_scan<const N: usize, R, F>(
    lisp: &Lisp<N>,
    env: ArenaIndex,
    name: ArenaIndex,
    mut f: F,
) -> ArenaResult<R>
where
    F: FnMut(ArenaIndex) -> ArenaResult<R>,
{
    let mut cur = env;
    while !cur.is_nil() {
        let binding = lisp.car(cur)?;
        if lisp.car(binding)? == name {
            return f(binding);
        }
        cur = lisp.cdr(cur)?;
    }
    Err(ArenaError::UnboundVariable)
}

/// Bind a name to a value in an environment, returning the new environment.
#[inline]
fn env_bind<const N: usize>(
    lisp: &Lisp<N>,
    env: ArenaIndex,
    name: ArenaIndex,
    val: ArenaIndex,
) -> ArenaResult<ArenaIndex> {
    let pair = lisp.cons(name, val)?;
    lisp.cons(pair, env)
}

/// Look up a name in an environment.
#[inline]
fn env_lookup<const N: usize>(
    lisp: &Lisp<N>,
    env: ArenaIndex,
    name: ArenaIndex,
) -> ArenaResult<ArenaIndex> {
    env_scan(lisp, env, name, |binding| lisp.cdr(binding))
}

/// Set a binding in an environment (mutate existing binding).
#[inline]
fn env_set<const N: usize>(
    lisp: &Lisp<N>,
    env: ArenaIndex,
    name: ArenaIndex,
    val: ArenaIndex,
) -> ArenaResult<()> {
    env_scan(lisp, env, name, |binding| {
        let key = lisp.car(binding)?;
        lisp.arena.set(binding, Value::Cons { car: key, cdr: val })
    })
}

impl<'a, const N: usize> Evaluator<'a, N> {
    /// Create a new evaluator with built-in functions bound in the global environment.
    pub fn new(lisp: &'a Lisp<N>) -> Self {
        let mut eval = Evaluator {
            lisp,
            global_env: ArenaIndex::NIL,
            gc_roots: ArenaIndex::NIL,
        };
        eval.init_builtins();
        eval
    }

    /// Push a value onto the GC root stack so it survives collection.
    ///
    /// The root stack is a linked list of cons cells in the arena:
    /// each entry is `(value . rest)`.
    #[inline]
    fn push_root(&mut self, idx: ArenaIndex) {
        if let Ok(new_roots) = self.lisp.cons(idx, self.gc_roots) {
            self.gc_roots = new_roots;
        } else {
            debug_assert!(false, "GC root push failed: arena out of memory");
        }
    }

    /// Pop `n` values from the GC root stack.
    #[inline]
    fn pop_roots(&mut self, n: usize) {
        for _ in 0..n {
            if self.gc_roots.is_nil() {
                break;
            }
            let rest = self.lisp.cdr(self.gc_roots);
            debug_assert!(rest.is_ok(), "GC root list corrupted during pop");
            if let Ok(rest) = rest {
                self.gc_roots = rest;
            } else {
                break;
            }
        }
    }

    /// Force a value to Weak Head Normal Form (WHNF), memoizing the result.
    ///
    /// Uses the poll-based protocol: `Lazy` → `Polling` → `Ready`.
    ///
    /// This is the low-level forcing loop. Prefer [`force_async`] which
    /// drives a [`ForceFuture`] via the [`block_on`] executor.
    #[allow(dead_code)]
    pub fn force(&mut self, mut idx: ArenaIndex) -> ArenaResult<ArenaIndex> {
        loop {
            let val = self.lisp.get(idx)?;

            if val.is_whnf() {
                return Ok(idx);
            }

            match val {
                // Ready — follow the memoized result pointer.
                Value::Ready(target) => idx = target,

                // Lazy — poll it via the Polling protocol.
                Value::Lazy { expr, env } => {
                    let lazy_idx = idx;
                    self.lisp.arena.set(lazy_idx, Value::Polling)?;
                    self.push_root(lazy_idx);

                    let result = self.eval(expr, env)?;
                    self.pop_roots(1);

                    // Follow ready chain, detecting cycles via polling sentinel.
                    let mut target = result;
                    loop {
                        match self.lisp.get(target)? {
                            Value::Ready(t) => target = t,
                            Value::Polling => return Err(ArenaError::BlackHoleDetected),
                            _ => break,
                        }
                    }

                    // Memoize: overwrite lazy cell with ready.
                    self.lisp.arena.set(lazy_idx, Value::Ready(target))?;
                    idx = target;
                }

                // Circular dependency detected.
                Value::Polling => return Err(ArenaError::BlackHoleDetected),

                _ => unreachable!(),
            }
        }
    }

    /// Force a value to WHNF using the [`ForceFuture`] poll-based protocol,
    /// driven by the [`block_on`] minimal executor.
    pub fn force_async(&mut self, idx: ArenaIndex) -> ArenaResult<ArenaIndex> {
        block_on(ForceFuture {
            evaluator: self,
            idx,
        })
    }

    /// Async wrapper around [`eval`]: evaluates an expression, returning the
    /// result.  This is a thin async fn that delegates to the synchronous
    /// `eval` method, providing an `.await`-compatible interface for callers
    /// that compose multiple async evaluation steps (e.g., `eval_force_async`).
    #[allow(dead_code)]
    pub async fn eval_async(
        &mut self,
        expr: ArenaIndex,
        env: ArenaIndex,
    ) -> ArenaResult<ArenaIndex> {
        self.eval(expr, env)
    }

    /// Async wrapper: evaluate an expression and force the result to WHNF.
    #[allow(dead_code)]
    pub async fn eval_force_async(
        &mut self,
        expr: ArenaIndex,
        env: ArenaIndex,
    ) -> ArenaResult<ArenaIndex> {
        let val = self.eval_async(expr, env).await?;
        self.force_async(val)
    }

    /// Async wrapper: evaluate and force all arguments in a list.
    /// Provides an `.await`-compatible interface for composing with other
    /// async evaluation functions.
    #[allow(dead_code)]
    pub async fn force_args_async(
        &mut self,
        args: ArenaIndex,
        env: ArenaIndex,
    ) -> ArenaResult<ArenaIndex> {
        self.force_args(args, env)
    }

    /// Trigger garbage collection using all known live roots.
    ///
    /// The roots include the global environment, the current expression
    /// and environment, and the GC root linked list (which the arena's
    /// mark phase will trace through automatically).
    fn collect_garbage(&self, expr: ArenaIndex, env: ArenaIndex) {
        self.lisp
            .arena
            .collect_garbage(&[expr, env, self.global_env, self.gc_roots]);
    }

    /// Check arena memory pressure and collect garbage if needed.
    ///
    /// Triggers GC when the arena is more than 75% full, using the
    /// provided expression and environment as additional GC roots
    /// alongside the global environment.
    #[inline]
    fn maybe_collect(&self, expr: ArenaIndex, env: ArenaIndex) {
        let len = self.lisp.arena.len();
        let cap = self.lisp.arena.capacity();
        if len > cap * 3 / 4 {
            self.collect_garbage(expr, env);
        }
    }

    /// Evaluate an expression and immediately force the result to WHNF.
    ///
    /// Uses [`force_async`] which drives the [`ForceFuture`] poll-based
    /// protocol via the minimal [`block_on`] executor.
    #[inline]
    fn eval_force(&mut self, expr: ArenaIndex, env: ArenaIndex) -> ArenaResult<ArenaIndex> {
        let val = self.eval(expr, env)?;
        self.force_async(val)
    }

    /// Evaluate an expression in an environment (with TCO).
    pub fn eval(&mut self, mut expr: ArenaIndex, mut env: ArenaIndex) -> ArenaResult<ArenaIndex> {
        loop {
            // Collect garbage when the arena is under memory pressure.
            self.maybe_collect(expr, env);

            let val = self.lisp.get(expr)?;

            // Self-evaluating: literals, closures, builtins.
            if val.is_self_evaluating() {
                return Ok(expr);
            }

            match val {
                // Lazy value encountered as a bare expression — force it.
                Value::Lazy { .. } => return self.force_async(expr),

                // Ready — follow the memoized result (no stack growth).
                Value::Ready(target) => {
                    expr = target;
                    continue;
                }

                // Polling — circular evaluation.
                Value::Polling => return Err(ArenaError::BlackHoleDetected),

                // Symbol → look up in local env, then global env.
                Value::Symbol(_) => {
                    let binding = env_lookup(self.lisp, env, expr)
                        .or_else(|_| env_lookup(self.lisp, self.global_env, expr))?;
                    if matches!(self.lisp.get(binding)?, Value::Polling) {
                        return Err(ArenaError::BlackHoleDetected);
                    }
                    return Ok(binding);
                }

                // List → special form or function application.
                Value::Cons { car, cdr } => {
                    self.push_root(cdr);
                    self.push_root(env);

                    // Check for special forms.
                    if matches!(self.lisp.get(car)?, Value::Symbol(_)) {
                        if let Some(action) =
                            self.try_special_form_tco(car, cdr, &mut expr, &mut env)
                        {
                            self.pop_roots(2);
                            match action {
                                TailAction::Return(val) => return val,
                                TailAction::Continue => continue,
                            }
                        }
                    }

                    // Function application (call-by-need).
                    let func_whnf = self.eval_force(car, env)?;

                    match self.lisp.get(func_whnf)? {
                        Value::Builtin(id) => {
                            let args = self.force_args(cdr, env)?;
                            self.pop_roots(2);
                            return self.apply_builtin(id, args);
                        }
                        Value::Lambda { .. } => {
                            let (params, body, closed_env) = self.lisp.lambda_parts(func_whnf)?;
                            env = self.bind_args_lazy(closed_env, params, cdr, env)?;
                            expr = body;
                            self.pop_roots(2);
                            continue; // ← TCO
                        }
                        _ => {
                            self.pop_roots(2);
                            return Err(ArenaError::NotCallable);
                        }
                    }
                }

                _ => unreachable!(),
            }
        }
    }

    /// Bind parameters to argument thunks (call-by-need).
    fn bind_args_lazy(
        &mut self,
        mut fn_env: ArenaIndex,
        mut params: ArenaIndex,
        mut arg_exprs: ArenaIndex,
        call_env: ArenaIndex,
    ) -> ArenaResult<ArenaIndex> {
        while !params.is_nil() && !arg_exprs.is_nil() {
            match self.lisp.get(params)? {
                Value::Cons {
                    car: param,
                    cdr: rest,
                } => {
                    let arg_expr = self.lisp.car(arg_exprs)?;
                    let lazy = self.lisp.lazy(arg_expr, call_env)?;
                    fn_env = env_bind(self.lisp, fn_env, param, lazy)?;

                    params = rest;
                    arg_exprs = self.lisp.cdr(arg_exprs)?;
                }
                Value::Symbol(_) => {
                    // Rest parameter: bind remaining args as a thunk-wrapped list
                    // We need to evaluate the rest args lazily.
                    // Build a list of thunks for the remaining args.
                    let rest_lazys = self.make_lazy_list(arg_exprs, call_env)?;
                    fn_env = env_bind(self.lisp, fn_env, params, rest_lazys)?;
                    return Ok(fn_env);
                }
                _ => return Err(ArenaError::TypeError),
            }
        }
        Ok(fn_env)
    }

    /// Build a list of lazy values from a list of expressions (iterative, O(n)).
    fn make_lazy_list(&self, exprs: ArenaIndex, call_env: ArenaIndex) -> ArenaResult<ArenaIndex> {
        let mut reversed = ArenaIndex::NIL;
        let mut cur = exprs;
        while !cur.is_nil() {
            let lazy = self.lisp.lazy(self.lisp.car(cur)?, call_env)?;
            reversed = self.lisp.cons(lazy, reversed)?;
            cur = self.lisp.cdr(cur)?;
        }
        // Reverse to restore original order.
        let mut result = ArenaIndex::NIL;
        while !reversed.is_nil() {
            let head = self.lisp.car(reversed)?;
            result = self.lisp.cons(head, result)?;
            reversed = self.lisp.cdr(reversed)?;
        }
        Ok(result)
    }

    /// Evaluate and force all arguments in a list (for strict builtins).
    fn force_args(&mut self, args: ArenaIndex, env: ArenaIndex) -> ArenaResult<ArenaIndex> {
        if args.is_nil() {
            return self.lisp.nil();
        }
        // Protect `args` and `env` across recursive eval/force calls.
        self.push_root(args);
        self.push_root(env);

        let head_expr = self.lisp.car(args)?;
        let head_forced = self.eval_force(head_expr, env)?;

        // Protect `head_forced` across the recursive force_args call.
        self.push_root(head_forced);

        let tail = self.lisp.cdr(args)?;
        let tail_forced = self.force_args(tail, env)?;

        self.pop_roots(3); // head_forced, env, args

        self.lisp.cons(head_forced, tail_forced)
    }

    // — Special forms (TCO-aware) —

    /// `(quote expr)` — return the expression unevaluated.
    fn eval_quote(
        &mut self,
        args: ArenaIndex,
        _expr: &mut ArenaIndex,
        _env: &mut ArenaIndex,
    ) -> TailAction {
        TailAction::Return(self.lisp.car(args))
    }

    /// `(if test then else)` — test is strict, branches are tail positions.
    fn eval_if(
        &mut self,
        args: ArenaIndex,
        expr: &mut ArenaIndex,
        env: &mut ArenaIndex,
    ) -> TailAction {
        tail_continue!((|| -> ArenaResult<()> {
            let test_expr = self.lisp.car(args)?;
            let rest = self.lisp.cdr(args)?;

            let test_forced = self.eval_force(test_expr, *env)?;

            if self.lisp.get(test_forced)?.is_truthy() {
                *expr = self.lisp.car(rest)?;
            } else {
                let else_rest = self.lisp.cdr(rest)?;
                *expr = if else_rest.is_nil() {
                    self.lisp.nil()?
                } else {
                    self.lisp.car(else_rest)?
                };
            }
            Ok(())
        })())
    }

    /// `(define name expr)` or `(define (name params...) body)`.
    fn eval_define(
        &mut self,
        args: ArenaIndex,
        _expr: &mut ArenaIndex,
        env: &mut ArenaIndex,
    ) -> TailAction {
        non_tail(self.eval_define_inner(args, *env))
    }

    fn eval_define_inner(&mut self, args: ArenaIndex, env: ArenaIndex) -> ArenaResult<ArenaIndex> {
        let first = self.lisp.car(args)?;
        let rest = self.lisp.cdr(args)?;

        match self.lisp.get(first)? {
            Value::Symbol(_) => {
                let val_expr = self.lisp.car(rest)?;
                // Create lazy value for the RHS
                let lazy = self.lisp.lazy(val_expr, env)?;
                self.global_env = env_bind(self.lisp, self.global_env, first, lazy)?;
                Ok(lazy)
            }
            Value::Cons {
                car: name,
                cdr: params,
            } => {
                // Function shorthand — lambda is already WHNF, no thunk needed
                let body = self.wrap_begin(rest)?;
                let lam = self.lisp.lambda(params, body, env)?;
                self.global_env = env_bind(self.lisp, self.global_env, name, lam)?;
                Ok(lam)
            }
            _ => Err(ArenaError::TypeError),
        }
    }

    /// `(set! name expr)`.
    fn eval_set(
        &mut self,
        args: ArenaIndex,
        _expr: &mut ArenaIndex,
        env: &mut ArenaIndex,
    ) -> TailAction {
        non_tail(self.eval_set_inner(args, *env))
    }

    fn eval_set_inner(&mut self, args: ArenaIndex, env: ArenaIndex) -> ArenaResult<ArenaIndex> {
        let name = self.lisp.car(args)?;
        let expr = self.lisp.car(self.lisp.cdr(args)?)?;
        let forced = self.eval_force(expr, env)?;
        env_set(self.lisp, env, name, forced)
            .or_else(|_| env_set(self.lisp, self.global_env, name, forced))?;
        self.lisp.nil()
    }

    /// `(lambda (params...) body...)`.
    fn eval_lambda(
        &mut self,
        args: ArenaIndex,
        _expr: &mut ArenaIndex,
        env: &mut ArenaIndex,
    ) -> TailAction {
        non_tail(self.eval_lambda_inner(args, *env))
    }

    fn eval_lambda_inner(&mut self, args: ArenaIndex, env: ArenaIndex) -> ArenaResult<ArenaIndex> {
        let params = self.lisp.car(args)?;
        let body_list = self.lisp.cdr(args)?;
        let body = self.wrap_begin(body_list)?;
        self.lisp.lambda(params, body, env)
    }

    /// `(begin expr1 expr2 ...)` — all but last are non-tail, last is tail.
    fn eval_begin(
        &mut self,
        args: ArenaIndex,
        expr: &mut ArenaIndex,
        env: &mut ArenaIndex,
    ) -> TailAction {
        tail_continue!((|| -> ArenaResult<()> {
            let mut cur = args;
            while !cur.is_nil() {
                let next = self.lisp.cdr(cur)?;
                if next.is_nil() {
                    *expr = self.lisp.car(cur)?;
                    return Ok(());
                }
                let e = self.lisp.car(cur)?;
                self.eval(e, *env)?;
                cur = next;
            }
            *expr = self.lisp.nil()?;
            Ok(())
        })())
    }

    /// `(cond (test expr...) ...)` — tests are strict, last body expr is tail.
    fn eval_cond(
        &mut self,
        args: ArenaIndex,
        expr: &mut ArenaIndex,
        env: &mut ArenaIndex,
    ) -> TailAction {
        tail_continue!((|| -> ArenaResult<()> {
            let mut cur = args;
            while !cur.is_nil() {
                let clause = self.lisp.car(cur)?;
                let test = self.lisp.car(clause)?;
                let body = self.lisp.cdr(clause)?;

                let matched = self.lisp.symbol_name_eq(test, "else") || {
                    let test_forced = self.eval_force(test, *env)?;
                    self.lisp.get(test_forced)?.is_truthy()
                };

                if matched {
                    *expr = self.wrap_begin(body)?;
                    return Ok(());
                }
                cur = self.lisp.cdr(cur)?;
            }
            *expr = self.lisp.nil()?;
            Ok(())
        })())
    }

    /// `(and expr1 expr2 ...)` — strict on tests, last is tail.
    fn eval_and(
        &mut self,
        args: ArenaIndex,
        expr: &mut ArenaIndex,
        env: &mut ArenaIndex,
    ) -> TailAction {
        self.eval_short_circuit(args, expr, env, true)
    }

    /// `(or expr1 expr2 ...)` — strict on tests, last is tail.
    fn eval_or(
        &mut self,
        args: ArenaIndex,
        expr: &mut ArenaIndex,
        env: &mut ArenaIndex,
    ) -> TailAction {
        self.eval_short_circuit(args, expr, env, false)
    }

    /// Shared `and`/`or` implementation.
    ///
    /// `continue_while_truthy`: when `true` (and), continues while tests are
    /// truthy and short-circuits on the first falsy value; defaults to `#t`.
    /// When `false` (or), continues while tests are falsy and short-circuits
    /// on the first truthy value; defaults to `#f`.
    fn eval_short_circuit(
        &mut self,
        args: ArenaIndex,
        expr: &mut ArenaIndex,
        env: &mut ArenaIndex,
        continue_while_truthy: bool,
    ) -> TailAction {
        tail_continue!((|| -> ArenaResult<()> {
            let mut cur = args;
            while !cur.is_nil() {
                let next = self.lisp.cdr(cur)?;
                if next.is_nil() {
                    *expr = self.lisp.car(cur)?;
                    return Ok(());
                }
                let e = self.lisp.car(cur)?;
                let forced = self.eval_force(e, *env)?;
                if self.lisp.get(forced)?.is_truthy() != continue_while_truthy {
                    *expr = forced;
                    return Ok(());
                }
                cur = next;
            }
            *expr = self.lisp.boolean(continue_while_truthy)?;
            Ok(())
        })())
    }

    /// `(let ((name val) ...) body...)` — bindings are lazy, body is tail.
    fn eval_let(
        &mut self,
        args: ArenaIndex,
        expr: &mut ArenaIndex,
        env: &mut ArenaIndex,
    ) -> TailAction {
        tail_continue!((|| -> ArenaResult<()> {
            let bindings = self.lisp.car(args)?;
            let body_list = self.lisp.cdr(args)?;

            let mut local_env = *env;
            let mut cur = bindings;
            while !cur.is_nil() {
                let binding = self.lisp.car(cur)?;
                let name = self.lisp.car(binding)?;
                let val_expr = self.lisp.car(self.lisp.cdr(binding)?)?;
                let lazy = self.lisp.lazy(val_expr, *env)?;
                local_env = env_bind(self.lisp, local_env, name, lazy)?;
                cur = self.lisp.cdr(cur)?;
            }

            *env = local_env;
            *expr = self.wrap_begin(body_list)?;
            Ok(())
        })())
    }

    /// `(cons a b)` — lazy cons: does NOT evaluate arguments (creates lazy values).
    fn eval_cons(
        &mut self,
        args: ArenaIndex,
        _expr: &mut ArenaIndex,
        env: &mut ArenaIndex,
    ) -> TailAction {
        non_tail(self.eval_cons_inner(args, *env))
    }

    fn eval_cons_inner(&mut self, args: ArenaIndex, env: ArenaIndex) -> ArenaResult<ArenaIndex> {
        let a_expr = self.lisp.car(args)?;
        let b_expr = self.lisp.car(self.lisp.cdr(args)?)?;
        let a_lazy = self.lisp.lazy(a_expr, env)?;
        let b_lazy = self.lisp.lazy(b_expr, env)?;
        self.lisp.cons(a_lazy, b_lazy)
    }

    /// Wrap a list of expressions in a `begin` form if there are multiple,
    /// or return the single expression if there's only one.
    fn wrap_begin(&self, exprs: ArenaIndex) -> ArenaResult<ArenaIndex> {
        if exprs.is_nil() {
            return self.lisp.nil();
        }
        let rest = self.lisp.cdr(exprs)?;
        if rest.is_nil() {
            // Single expression, no need to wrap
            return self.lisp.car(exprs);
        }
        // Multiple expressions, wrap in begin
        let begin_sym = self.lisp.symbol("begin")?;
        self.lisp.cons(begin_sym, exprs)
    }

    // — Arithmetic built-ins —

    /// `(+ ...)` — variadic addition.
    fn builtin_add(&self, args: ArenaIndex) -> ArenaResult<ArenaIndex> {
        fold_numbers!(self, args, 0, checked_add)
    }

    /// `(- a b ...)` — subtraction. With one arg, negates.
    fn builtin_sub(&self, args: ArenaIndex) -> ArenaResult<ArenaIndex> {
        if args.is_nil() {
            return Err(ArenaError::InvalidArgument);
        }
        let first = self.lisp.get(self.lisp.car(args)?)?.as_number()?;
        let rest = self.lisp.cdr(args)?;
        if rest.is_nil() {
            return self
                .lisp
                .number(first.checked_neg().ok_or(ArenaError::ArithmeticOverflow)?);
        }
        fold_numbers!(self, rest, first, checked_sub)
    }

    /// `(* ...)` — variadic multiplication.
    fn builtin_mul(&self, args: ArenaIndex) -> ArenaResult<ArenaIndex> {
        fold_numbers!(self, args, 1, checked_mul)
    }

    /// `(/ a b)` — integer division.
    fn builtin_div(&self, args: ArenaIndex) -> ArenaResult<ArenaIndex> {
        let (a, b) = binary_nums!(self, args);
        if b == 0 {
            return Err(ArenaError::DivisionByZero);
        }
        self.lisp.number(a / b)
    }

    cmp_builtin!(builtin_eq, ==);
    cmp_builtin!(builtin_lt, <);
    cmp_builtin!(builtin_gt, >);
    cmp_builtin!(builtin_le, <=);
    cmp_builtin!(builtin_ge, >=);

    // — Pair / list built-ins —

    /// `(list ...)` — return args as-is (already forced into a list).
    fn builtin_list(&self, args: ArenaIndex) -> ArenaResult<ArenaIndex> {
        Ok(args)
    }

    // `(car pair)` / `(cdr pair)` — extract and force a pair component.
    pair_builtin!(builtin_car, car);
    pair_builtin!(builtin_cdr, cdr);

    // — Type predicate built-ins —

    type_predicate!(builtin_nullp, Value::Nil);
    type_predicate!(builtin_not, Value::Boolean(false));
    type_predicate!(builtin_pairp, Value::Cons { .. });
    type_predicate!(builtin_numberp, Value::Number(_));
    type_predicate!(builtin_symbolp, Value::Symbol(_));
    type_predicate!(builtin_booleanp, Value::Boolean(_));
}
