# Compiler Roadmap

JapalityBean now has a real compiler MVP, not just a syntax idea. The remaining work is a staged path from MVP to systems-language compiler.

## Stage 0: Current MVP

Implemented:

- `.jb` parser and AST
- independent lexer diagnostics
- JSON diagnostic emitter
- basic semantic and type checker
- canonical formatter
- cross-file project checking through `jb.toml`
- built-in standard signature table
- pseudo LLVM IR emission

## Stage 1: Strict Parser

Replace the current line parser with a token-stream parser backed by `src/lexer.rs`.

Required work:

- Parse all grammar from tokens instead of line strings.
- Preserve exact token spans for every AST node.
- Add parser recovery around newline, `@end`, and `@func` boundaries.
- Add parser tests for malformed source.

## Stage 2: Full Type System

Required work:

- Explicit cast parsing with `as::<T>`.
- `Option<T>` constructors and usage checks.
- `Result<T,E>` constructors and usage checks.
- Numeric cast table.
- Type context propagation for `None`, `Some`, `Ok`, and `Err`.

## Stage 3: Real LLVM Backend

Required work:

- Add `inkwell` or direct LLVM text lowering.
- Lower arithmetic, comparisons, calls, locals, returns, loops, and guards.
- Link a tiny C/Rust runtime ABI.
- Validate generated IR with `llvm-as` and `opt -verify`.

## Stage 4: Runtime And Standard Library

Required work:

- Implement allocation functions.
- Implement debug functions.
- Implement string and vector constructors.
- Add bounds checks and structured panic payloads.

## Stage 5: Toolchain Quality

Required work:

- Unit tests for lexer/parser/type checker.
- Golden JSON diagnostics.
- Round-trip formatter tests.
- Cross-file module tests.
- Property tests for parser/formatter stability.
- CI script.

## Stage 6: Self-Hosting Direction

Only after the LLVM backend is stable:

- Write parts of the standard library in JapalityBean.
- Build a small interpreter for compile-time tests.
- Eventually rewrite selected compiler passes in JapalityBean if the language becomes expressive enough.
