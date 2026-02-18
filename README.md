# grift

a `#![no_std]` lisp interpreter implementing the vau calculus (Shutt 2010) on a fixed-size arena allocator with mark-and-sweep garbage collection.

## Features

- `no_std`, `no_alloc`, `#![forbid(unsafe_code)]`
- Fixed-size arena with free-list allocation and contiguous string storage
- Vau calculus: first-class operatives (fexprs) subsume both functions and macros
- First-class mutable environments with lexical parent chains
- Applicative/operative combiner distinction (Kernel-style)
- Tail-call optimization via trampoline
- Mark-and-sweep garbage collection with shadow root stack
- Immutable pairs, call-by-value evaluation
- Symbol interning
- Checked integer arithmetic

## Usage (Rust API)

```rust
use grift::{Lisp, Value};

let lisp: Lisp<20000> = Lisp::new();

assert_eq!(lisp.eval("(+ 1 2)"), Ok(Value::Number(3)));
assert_eq!(lisp.eval("(car (cons 1 2))"), Ok(Value::Number(1)));
```

The const generic parameter (`20000`) sets the arena capacity in slots.

## Usage (Lisp)

Grift's distinguishing feature is the `vau` operative, which receives its operands
unevaluated along with the caller's environment. `lambda` is derived from `vau`
and `wrap`:

```lisp
;; vau creates an operative (fexpr) — operands are not evaluated
(define! my-quote (vau (x) #ignore x))
(my-quote (+ 1 2))   ; → (+ 1 2), not 3

;; lambda is sugar for (wrap (vau params #ignore body))
(define! double (lambda (x) (* x 2)))
(double 5)            ; → 10

;; An operative that selectively evaluates using the caller's environment
(define! my-if (vau (test then else) e
  (if (eval test e)
    (eval then e)
    (eval else e))))

;; Tail-recursive fibonacci
(define! fib (lambda (n a b)
  (if (= n 0) a
    (fib (- n 1) b (+ a b)))))
(fib 50 0 1)          ; → 12586269025
```

## Architecture

Grift stores all values in a flat `[Cell<Slot<Value>>; N]` array. Each `Value`
variant fits in two machine words plus a tag. The evaluator is a trampoline loop:
tail-position combiners update `expr` and `env` and re-enter the loop rather than
recursing. Garbage collection uses a mark bitmap and mark stack allocated on the
Rust stack (not the arena), triggered when occupancy exceeds 75%. Environments are
the sole mutable type — pairs, symbols, strings, and combiners are all immutable
once allocated. See [ARCHITECTURE.md](ARCHITECTURE.md) for full details.

## Build

```bash
cargo build --workspace
cargo test --workspace
```

Run the benchmark suite:

```bash
cargo run -p grift --example fib_bench --release
```

Run the REPL (requires the `repl` feature):

```bash
cargo run -p grift --features repl
```

## Tooling

### Interactive REPL

The REPL provides a rich interactive environment with meta-commands:

```
Λ> ,help              Show available commands
Λ> ,builtins          List all built-in operatives and applicatives
Λ> ,doc lambda        Show documentation for a builtin
Λ> ,check (+ 1 2)    Run static analysis on an expression
Λ> ,env               Show arena allocation statistics
Λ> ,quit              Exit the REPL
```

### CLI Commands

```bash
grift help             # Show usage information
grift run <file>       # Execute a Grift source file
grift check <src>      # Run static analysis on a file or expression
```

### Static Analysis (`grift_check`)

The `grift_check` crate provides standalone static analysis:

- Bracket matching (unmatched parentheses)
- String literal validation (unterminated strings)
- Position tracking for all diagnostics

```rust
use grift_check::check;

let result = check("(+ 1 2)");
assert!(result.is_ok());

let result = check("(+ 1 2");
assert!(!result.is_ok()); // reports unmatched parenthesis
```

### Language Server (`grift_lsp`)

The `grift_lsp` crate provides an LSP server for IDE integration:

- **Diagnostics**: Parse error detection with position information
- **Hover**: Documentation for all 37 built-in operatives and applicatives
- **Completion**: Auto-complete for all builtins and keywords

Run the LSP server:

```bash
cargo run -p grift_lsp
```

Configure in your editor (e.g., VS Code `settings.json`):

```json
{
  "grift.lsp.path": "path/to/grift-lsp"
}
```

### API Documentation

Generate comprehensive HTML documentation for all crates:

```bash
cargo doc --workspace --open
```

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) -- System architecture, arena design, GC, TCO
- [LANGUAGE.md](LANGUAGE.md) -- Language reference: types, primitives, evaluation rules
- [INTERNALS.md](INTERNALS.md) -- Contributor guide: module structure, adding builtins

## License

MIT OR Apache-2.0
