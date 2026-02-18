# Changelog

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
