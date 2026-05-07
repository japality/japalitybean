# JapalityBean LLM Authoring Guide

This guide is written for local LLMs generating JapalityBean source. It is intentionally strict and example-heavy because JapalityBean is not in most model pretraining data yet.

## Non-Negotiable Rules

- Always emit a complete function block: `@func`, `@intent`, all `@in`, one `@out`, `---`, body, and matching `@end func name`.
- Every introduced binding must use sticky typing: `let name::Type = expr`.
- The `@out` binding is already declared by `@out result::Type`. Never write `let result::Type = ...`; use `set result = ...`.
- Every loop item must use sticky typing: `@loop item::i32 in nums`.
- Never use braces, semicolons, square brackets, imports, or traditional multi-line `if` blocks.
- Never write `let x = if (...) ...`. JapalityBean does not have expression-level `if`.
- All conditional control flow must be a two-line guard:

```japalitybean
@condition label
if (condition) -> action
```

- Valid guard actions are only `@continue`, `@break`, `return expr`, or `set name = expr`.
- Prefer early guard chains over `else`.

## Scalar Pattern

Use a mutable result binding plus sequential guards.

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

## Loop Filter Pattern

Use one guard per reason to skip or update. Do not combine multiple actions inside one condition.

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

## Vector Constructor Pattern

Native Linux executables currently support the fixed built-in `vector_i32_3`.

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

## Common Anti-Patterns

Do not generate these:

```japalitybean
let total = 0
let result::i32 = 0
@loop item in nums
if (x < low) { set result = low }
let result::i32 = if (x < low) -> low
nums[0]
```

Generate these instead:

```japalitybean
let total::i32 = 0
set result = 0
@loop item::i32 in nums
@condition below_low
if (x < low) -> set result = low
set result = x
```

## Repair Prompt Template

When `jbc check --json-batch` fails, send the model this:

```text
Fix this JapalityBean source so jbc check passes.
Return ONLY complete corrected source.

Rules:
- Every let must be `let name::Type = expr`.
- Every loop must be `@loop item::i32 in collection`.
- No braces, semicolons, imports, square brackets, or expression-level if.
- Conditions must be:
  @condition label
  if (expr) -> action

Previous source:
<source>

Compiler JSON diagnostics:
<json diagnostics>
```
