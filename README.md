# JapalityBean

JapalityBean is an experimental LLM-first systems language concept by Japality Limited.

The project explores whether a programming language can be made easier for large language models to read, reason about, generate, and repair while still moving toward native compiler behavior similar in spirit to C, C++, Go, or Rust.

This repository is a clean public concept snapshot. The current internal compiler prototype is under active development and has already demonstrated parsing, type checking, structured JSON diagnostics, LLVM text IR emission, and direct Linux x86_64 executable emission for a focused subset of the language.

## Design Direction

- Explicit `@` tags for program structure.
- Intent-first function declarations through `@intent:`.
- Sticky typing with `name::Type`.
- Named block closure such as `@end func name`.
- Guard-style conditions to reduce deeply nested control flow.
- JSON diagnostics intended for LLM repair loops.

## Minimal Example

```japalitybean
@func clamp_i32
@intent: Clamp an i32 into inclusive bounds
@in x::i32
@in low::i32
@in high::i32
@out result::i32
---
set result = x
@condition below_low
if (x < low) -> set result = low
@condition above_high
if (x > high) -> set result = high
return result
@end func clamp_i32
```

## Current Status

JapalityBean is currently a research prototype, not a production-ready compiler toolchain.

The internal prototype test run on 2026-05-07 passed:

- `cargo test`: 13/13 integration tests passed.
- Native Linux smoke tests: passed for selected examples.
- JSON diagnostics: emitted valid machine-readable error reports.
- Preliminary LLM generation sample: one small JapalityBean task passed after tightening the authoring guide.

See `docs/test-report-2026-05-07.md` for details and limitations.

## Repository Contents

- `docs/language-sketch-v0.2.md`: current public language sketch.
- `docs/llm-authoring-guide.md`: strict generation guide for LLMs.
- `docs/test-report-2026-05-07.md`: current local testing report.
- `examples/`: small JapalityBean examples.

## License

MIT License. Copyright (c) 2026 Japality Limited.
