# JapalityBean Manual

This manual teaches the current public JapalityBean language prototype and the `jbc` compiler workflow.

JapalityBean is experimental. The syntax and compiler behavior may change as the project evolves.

## 1. Core Idea

JapalityBean is designed around explicit structure:

- Functions start with `@func` and end with `@end func name`.
- Function purpose is declared up front with `@intent:`.
- Inputs and outputs are declared before the body.
- Types stick to names with `name::Type`.
- Loops and conditions use tags instead of braces.
- Diagnostics can be emitted as JSON for LLM repair loops.

The goal is to make source code easier for both humans and LLMs to inspect and repair.

## 2. Build The Compiler

Install Rust, then run:

```sh
cargo build
```

Run the compiler through Cargo:

```sh
cargo run -- check examples/find_max_even.jb --json-batch
```

Or run the built binary directly:

```sh
target/debug/jbc check examples/find_max_even.jb --json-batch
```

## 3. A Complete Function

Every useful JapalityBean file contains one or more complete function blocks.

```japalitybean
@func add_i32
@intent: Add two signed 32-bit integers
@in left::i32
@in right::i32
@out result::i32
---
set result = left + right
return result
@end func add_i32
```

The required order is:

1. `@func name`
2. `@intent: text`
3. zero or more `@in name::Type` lines
4. one `@out name::Type` line
5. `---`
6. body statements
7. `@end func name`

The function close name must match the open name.

## 4. Sticky Types

JapalityBean uses sticky type declarations:

```japalitybean
@in seed::i32
let total::i32 = 0
@loop item::i32 in nums
```

Do not write untyped bindings:

```japalitybean
let total = 0
@loop item in nums
```

The `@out` binding is already declared by the function header. Assign to it with `set`:

```japalitybean
@out result::i32
---
set result = 42
return result
```

Do not redeclare the output:

```japalitybean
let result::i32 = 42
```

## 5. Types

The current language accepts these type names:

- Signed integers: `i8`, `i16`, `i32`, `i64`
- Unsigned integers: `u8`, `u16`, `u32`, `u64`
- Floats: `f32`, `f64`
- Other primitives: `bool`, `string`, `unit`
- Generic shapes: `Vector<T>`, `Option<T>`, `Result<T,E>`, `Box<T>`

The native Linux backend currently supports a smaller runtime subset than the type checker. For native executable output, prefer `i32`, `bool`, string literals used with `debug_string`, and `Vector<i32>` through `vector_i32_3`.

## 6. Statements

### `let`

Use `let` to create a new local binding.

```japalitybean
let doubled::i32 = seed + seed
```

### `set`

Use `set` to update an existing binding.

```japalitybean
set result = doubled
```

### `return`

Use `return` to leave the function.

```japalitybean
return result
```

For `unit` functions, a bare `return` is accepted by the grammar.

## 7. Expressions

The current expression parser supports:

- integer, float, string, and boolean literals
- identifiers
- unary `-` and `!`
- arithmetic: `+`, `-`, `*`, `/`, `%`
- comparisons: `<`, `<=`, `>`, `>=`
- equality: `==`, `!=`
- boolean operators: `&&`, `||`
- function calls: `name(arg1, arg2)`
- casts: `expr as::<Type>`
- constructors: `Some(value)`, `None`, `Ok(value)`, `Err(value)`
- parentheses: `(expr)`

Example:

```japalitybean
let abs_seed::i32 = abs_i32(seed)
let widened::i64 = abs_seed as::<i64>
```

JapalityBean intentionally rejects implicit numeric widening in the type checker. Use an explicit cast when a wider type is required.

## 8. Conditions

Conditions are two-line guard blocks.

```japalitybean
@condition negative
if (seed < 0) -> set result = 0
```

Valid guard actions are:

- `@continue`
- `@break`
- `return expr`
- `set name = expr`

Do not use braces or traditional multi-line `if` bodies:

```japalitybean
if (seed < 0) { set result = 0 }
```

Use a guard instead:

```japalitybean
@condition negative
if (seed < 0) -> set result = 0
```

## 9. Loops

Loops iterate over a named collection with a typed loop item.

```japalitybean
@func sum_positive_even
@intent: Sum positive even numbers
@in nums::Vector<i32>
@out result::i32
---
let total::i32 = 0
@loop item::i32 in nums
  @condition skip_non_positive
  if (item <= 0) -> @continue
  @condition skip_odd
  if (item % 2 != 0) -> @continue
  @condition add_value
  if (item >= 0) -> set total = total + item
@end loop nums
set result = total
return result
@end func sum_positive_even
```

The loop close name is optional in the grammar, but naming it is recommended for readability.

## 10. Built-In Functions

The current compiler installs these built-in signatures:

```text
debug_i32(value::i32) -> unit
debug_string(value::string) -> unit
abs_i32(value::i32) -> i32
is_even_i32(value::i32) -> bool
max_i32(left::i32, right::i32) -> i32
vector_len_i32(items::Vector<i32>) -> i64
vector_i32_3(first::i32, second::i32, third::i32) -> Vector<i32>
```

Example:

```japalitybean
@func vector_sum_demo
@intent: Construct and sum a small native vector
@in seed::i32
@out result::i32
---
let nums::Vector<i32> = vector_i32_3(seed, seed + 2, 9)
let total::i32 = 0
@loop item::i32 in nums
  @condition add_all
  if (item >= 0) -> set total = total + item
@end loop nums
set result = total
return result
@end func vector_sum_demo
```

## 11. Comments

Whole-line comments start with `#`.

```japalitybean
# This function returns a Linux process exit code.
@func main
@intent: Return a Linux process exit code
@in seed::i32
@out result::i32
---
set result = seed + 7
return result
@end func main
```

Inline comments are not part of the recommended style for the current prototype.

## 12. Project Mode

A single `.jb` file can be checked directly:

```sh
jbc check examples/find_max_even.jb --json-batch
```

A directory with `jb.toml` is treated as a project. The compiler reads `.jb` files under `src/` and resolves functions in one flat namespace.

Example layout:

```text
examples/project/
  jb.toml
  src/
    main.jb
    math.jb
```

Check a project:

```sh
jbc check examples/project --json-batch
```

## 13. Native Executables

The compiler can emit direct Linux x86_64 ELF executables for the supported native subset.

```sh
cargo run -- build examples/linux_main.jb --emit=exe -o /tmp/jb-main
/tmp/jb-main 10
echo $?
```

The executable entry function defaults to `main`. Use `--entry` to choose another function:

```sh
cargo run -- build examples/find_max_even.jb --emit=exe --entry find_max_even -o /tmp/jb-loop
```

Current native executable support includes:

- `i32` and `bool` local values
- arithmetic and comparison expressions
- assignments and returns
- function calls
- guard conditions
- `Vector<i32>` loops
- `vector_i32_3`
- selected built-ins such as `debug_i32`, `debug_string`, `abs_i32`, `is_even_i32`, and `max_i32`
- integer entry arguments read from `argv`

## 14. Diagnostics

Use `--json-batch` when you want machine-readable diagnostics:

```sh
cargo run -- check examples/type_error.jb --json-batch
```

Diagnostics include fields such as:

- `code`
- `severity`
- `phase`
- `node_id`
- `span`
- `message`
- `expected`
- `actual`
- `suggested_fix`

This format is intended to support automated repair loops with LLMs.

## 15. Common Mistakes

Do not use braces:

```japalitybean
if (x < 0) { return 0 }
```

Use a guard:

```japalitybean
@condition below_zero
if (x < 0) -> return 0
```

Do not omit sticky types:

```japalitybean
let total = 0
@loop item in nums
```

Use sticky types:

```japalitybean
let total::i32 = 0
@loop item::i32 in nums
```

Do not redeclare `result` after `@out result::Type`:

```japalitybean
let result::i32 = 0
```

Assign it:

```japalitybean
set result = 0
```

## 16. Formatting

Run the formatter:

```sh
cargo run -- fmt examples/find_max_even.jb
```

The formatter normalizes indentation to the current canonical style.

## 17. Current Limitations

The public compiler is a prototype. Important unfinished areas include:

- stable package and module design
- broader standard library
- heap allocation
- richer strings and vectors
- complete `Option` and `Result` runtime operations
- broader ABI support
- non-Linux and non-x86_64 native backends

Use the current version as an experimental compiler and language-design reference, not as a production systems toolchain.
