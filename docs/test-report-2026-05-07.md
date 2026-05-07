# JapalityBean Test Report - 2026-05-07

This report summarizes the current local test state of the JapalityBean compiler prototype.

## Environment

- OS: Linux 6.17.0-22-generic
- Compiler implementation language: Rust
- Prototype command: `jbc`
- Report date: 2026-05-07

## Summary

The internal prototype currently supports a focused compiler pipeline:

- Lexing and parsing for the current tag-based syntax.
- Sticky type checking for function inputs, outputs, locals, loops, assignments, calls, casts, and selected constructors.
- Structured JSON diagnostics for LLM repair workflows.
- AST JSON emission.
- LLVM text IR emission for a subset of programs.
- Direct Linux x86_64 executable emission for selected `i32`, loop, condition, function-call, vector, string, and debug-output examples.

This is not yet a complete Rust/C++/Go-level compiler. It is an early systems-language compiler prototype used to validate the LLM-first language design.

## Automated Tests

`cargo test` result:

```text
13 passed; 0 failed; 0 ignored
```

The passing tests cover:

- Valid example checking with empty JSON diagnostic batch.
- Machine-readable type error diagnostics.
- AST JSON output validity.
- LLVM text IR emission for loop examples.
- Formatter idempotence.
- Project mode and hidden file filtering.
- Casts, `Option`, and `Result` constructors.
- Rejection of implicit numeric widening.
- Direct Linux executable generation and execution.
- Native function calls, built-ins, conditions, and loops.
- Runtime `argv` integer input and `debug_i32` output.
- Static string literal output.
- Stack-backed `Vector<i32>` construction and iteration.

## Smoke Tests

The local smoke script completed successfully.

It checked:

- `jbc check` on valid examples.
- JSON diagnostic output on a known type error.
- AST JSON and LLVM text IR build paths.
- Native executable output for selected Linux examples.
- Execution of generated Linux binaries for calls, conditions, loops, runtime input, strings, and vectors.

## Preliminary LLM Suitability Check

A small local Ollama sample was used to test whether tighter JapalityBean authoring rules improve LLM generation.

Observed result:

- Model used locally: `llama3.1:8b`
- Task sample: `clamp_i32`
- After adding stricter rules to the LLM authoring guide, the sample passed `jbc check`.

This is only a preliminary signal, not a statistically meaningful benchmark. More repetitions, more models, and cross-language comparisons are still required before making strong claims.

## Known Limitations

- The native backend is currently Linux x86_64 focused.
- The implemented native subset is intentionally narrow.
- There is no stable standard library yet.
- Memory management, modules, packages, generics, and broader runtime design remain future work.
- Public syntax and compiler internals may change.

## Current Conclusion

The prototype is sufficient to demonstrate the core concept: a language with explicit structure, sticky typing, named closures, and structured diagnostics can support an LLM-oriented compiler workflow while still moving toward native executable generation.
