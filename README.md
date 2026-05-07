# JapalityBean

JapalityBean is an experimental LLM-first systems language and compiler by Japality Limited. The compiler binary is `jbc`.

The project explores whether a programming language can be made easier for large language models to read, reason about, generate, and repair while still moving toward native compiler behavior similar in spirit to C, C++, Go, or Rust.

This repository contains the current public compiler prototype.

## What `jbc` Can Do

- `jbc check <path>` parses and type-checks `.jb` files.
- `jbc fmt <path>` rewrites source into the canonical two-space format.
- `jbc build <path> --emit=ast-json` emits a compact AST JSON summary.
- `jbc build <path> --emit=ir` emits LLVM text IR.
- `jbc build <path> --emit=obj -o <file.o>` compiles LLVM IR to an object file through `clang`, `cc`, or `gcc`.
- `jbc build <path> --emit=exe -o <app>` emits a native Linux x86_64 ELF executable directly, without `clang`, `cc`, `gcc`, or a system linker.
- `--json` emits one structured diagnostic per line.
- `--json-batch` emits diagnostics as one JSON array.
- Project mode is enabled by `jb.toml`; all `.jb` files under `src/` share one flat function namespace.

## Language Shape

```japalitybean
@func find_max_even
@intent: Find the maximum even number in a given array
@in nums::Vector<i32>
@out result::i32
---
let max_even::i32 = -1

@loop item::i32 in nums
  @condition is_even
  if (item % 2 != 0) -> @continue

  @condition is_greater
  if (item > max_even) -> set max_even = item
@end loop nums

set result = max_even
return result
@end func find_max_even
```

## Build

Install Rust, then run:

```sh
cargo build
```

The compiler binary will be available at:

```sh
target/debug/jbc
```

## Try It

```sh
cargo run -- check examples/find_max_even.jb --json-batch
cargo run -- check examples/type_error.jb --json-batch
cargo run -- check examples/project --json-batch
cargo run -- build examples/find_max_even.jb --emit=ast-json
cargo run -- build examples/find_max_even.jb --emit=ir
```

Build and run a native Linux executable:

```sh
cargo run -- build examples/linux_main.jb --emit=exe -o /tmp/jb-main
/tmp/jb-main 10
echo $?
```

Or run the smoke script:

```sh
./scripts/smoke.sh
```

## LLM Benchmark

Use a local Ollama model to measure how well an LLM generates JapalityBean compared with other languages:

```sh
scripts/ollama_llm_bench.py --model llama3.1:8b --langs japalitybean,c,rust --reps 2 --max-repairs 2 --output /tmp/jb-ollama-bench.json
```

For quick iteration on one task:

```sh
scripts/ollama_llm_bench.py --model llama3.1:8b --langs japalitybean --tasks clamp_i32 --reps 1 --max-repairs 1
```

The benchmark uses `docs/llm-authoring-guide.md` and validates JapalityBean output with `jbc check --json-batch`.

## Docs

- `docs/language-v0.2.md`
- `docs/llm-authoring-guide.md`
- `docs/runtime-memory-model.md`
- `docs/compiler-roadmap.md`
- `docs/test-report-2026-05-07.md`

## Current Scope

JapalityBean is currently a research prototype, not a production-ready compiler toolchain.

The MVP intentionally focuses on the part that makes JapalityBean different: explicit structure, named closures, sticky typing, cross-file checking, and JSON diagnostics for LLM repair loops.

The native backend emits Linux x86_64 ELF directly and supports integer/bool locals, assignments, returns, casts, arithmetic/comparison expressions, user function calls, selected built-ins (`debug_i32`, `debug_string` for string literals, `abs_i32`, `is_even_i32`, `max_i32`, `vector_len_i32`, `vector_i32_3`), condition guards, stack-backed `Vector<i32>` construction, and `Vector<i32>` loops. Native executable entry `i32` inputs are read from argv.

Remaining work includes heap-backed allocation, richer strings/vectors, Option/Result runtime layout operations, modules/packages, a broader standard library, and more backend coverage.

## License

MIT License. Copyright (c) 2026 Japality Limited.
