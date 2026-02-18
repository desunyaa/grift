# Changelog

## 2026-02-18 (2)

### Improved — LSP and Static Checking

- **LSP diagnostics now use `grift_check`**: The language server previously
  reimplemented its own parse checking by running `lisp.eval()`, which created
  a full interpreter instance and reported all errors at line 0 with no column
  information. Now `check_parse()` runs `grift_check::check()` first, which
  provides accurate line/column positions for unmatched parentheses and
  unterminated strings. Eval-based checking is still used as a second pass for
  runtime errors (unbound variables, type errors) but only after structural
  checks pass.

- **LSP `textDocument/didSave` support**: Added `DidSaveParams` type and
  `did_save()` handler. The server now advertises save support with
  `includeText: true` in its capabilities. Editors that only publish diagnostics
  on save (e.g. some Vim/Neovim configurations) now get proper feedback.

- **LSP `textDocument/signatureHelp`**: New feature — when the cursor is inside
  a form like `(cons |)`, the server returns the signature `(cons a b)` with
  documentation. Uses a new `enclosing_call_symbol()` utility that walks
  backwards from the cursor to find the innermost unmatched `(` and extracts
  the head symbol. Works with nested expressions.

- **`grift_check` form arity validation**: Added `check_form_arity()` which
  warns when well-known forms like `define!`, `lambda`, `vau`, `if`, `set!`,
  and `let` are called with fewer arguments than required. For example,
  `(define!)` and `(define! x)` now produce arity warnings. This is a
  token-level check that counts top-level arguments inside parentheses,
  respecting nesting, strings, and comments.

- **21 new tests**: 8 new tests for `grift_check` (arity validation) and
  13 new tests for `grift_lsp` (grift_check integration, didSave, signature
  help, enclosing_call_symbol, capability advertisement).

## 2026-02-18

### Fixed
- **CI workflow**: `no-std-check` job referenced non-existent crates
  (`grift_core`, `grift_parser`, `grift_eval`). Corrected to the actual
  workspace crate names (`grift_arena`, `grift`). Without this fix, the
  bare-metal verification job would always fail when CI runs.

- **Clippy warnings in `grift_lsp`**: Replaced `io::Error::new(io::ErrorKind::Other, e)`
  with `io::Error::other(e)` (idiomatic since Rust 1.74). Collapsed nested
  `if let` chains into single `let`-chains. Converted `loop`/`match`/`break`
  to `while let` in the LSP main loop.

- **Formatting**: Applied `cargo fmt` across the entire workspace.
  All files now pass `cargo fmt --all -- --check`.
