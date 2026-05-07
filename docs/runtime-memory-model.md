# Runtime And Memory Model

This document defines the intended runtime direction for JapalityBean. The current compiler checks the language and emits pseudo IR; this model guides the real LLVM backend.

## Memory Ownership

JapalityBean v1 is intentionally simpler than Rust:

- Primitive values are copied by value.
- `Vector<T>`, `string`, `Box<T>`, `Option<T>`, and `Result<T,E>` are owned values.
- Assignment moves owned values unless the type is primitive.
- Borrow checking is not part of v1.
- Shared references, mutable references, and lifetimes are reserved for a later version.

The goal is to avoid hidden aliasing rules while keeping a systems-language path open.

## Runtime Layout

Planned LLVM-compatible layouts:

- `bool` -> `i1`
- signed and unsigned integers -> fixed-width LLVM integers
- `f32` -> `float`
- `f64` -> `double`
- `string` -> `{ i8*, i64 }`
- `Vector<T>` -> `{ T*, i64, i64 }`
- `Option<T>` -> `{ i1, T }`
- `Result<T,E>` -> `{ i1, { T, E } }`
- `Box<T>` -> `T*`

## Allocation

The first real backend should provide a tiny runtime ABI:

```text
jb_alloc(size::u64, align::u64) -> *u8
jb_free(ptr::*u8, size::u64, align::u64) -> unit
jb_panic(message::string) -> unit
jb_debug_i32(value::i32) -> unit
jb_debug_string(value::string) -> unit
```

This keeps the compiler backend small while making allocation and diagnostics explicit.

## Failure Model

JapalityBean should prefer typed failure over exceptions:

- Recoverable failure uses `Result<T,E>`.
- Nullable values use `Option<T>`.
- Unrecoverable compiler/runtime traps call `jb_panic`.

No exceptions, stack unwinding, or implicit panic conversions are planned for v1.

## LLM-Friendly Constraint

Every ownership-affecting operation should eventually be visible in AST or IR:

- No hidden destructor magic in the source language.
- No overloaded copy/move behavior.
- No implicit heap allocation except constructors documented in the standard library.

This is stricter than many human-first languages, but it is easier for LLMs to reason about.
