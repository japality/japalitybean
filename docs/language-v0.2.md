# JapalityBean Language v0.2

JapalityBean is a new LLM-first systems language. Source files use the `.jb` extension and are compiled by `jbc`.

## Goals

- Make structure explicit through `@` tags.
- Require named closure for long-lived blocks.
- Keep type information sticky to identifiers with `name::Type`.
- Emit structured JSON diagnostics that an LLM can consume and repair.
- Prefer early return and single-action guards over nested branch trees.

## Core Syntax

```ebnf
Program        = { FunctionDecl } EOF ;
FunctionDecl   = "@func" Ident Newline
                 IntentPreamble
                 "---" Newline
                 { Statement }
                 "@end" "func" Ident Newline ;

IntentPreamble = "@intent:" Text Newline
                 { "@in" StickyBinding Newline }
                 "@out" StickyBinding Newline ;

StickyBinding  = Ident "::" Type ;

Statement      = LetStmt
               | SetStmt
               | ReturnStmt
               | LoopBlock
               | ConditionBlock
               | Expr ;

LetStmt        = "let" StickyBinding "=" Expr Newline ;
SetStmt        = "set" Ident "=" Expr Newline ;
ReturnStmt     = "return" [ Expr ] Newline ;

LoopBlock      = "@loop" StickyBinding "in" Ident Newline
                 { Statement }
                 "@end" "loop" [ Ident ] Newline ;

ConditionBlock = "@condition" Ident Newline
                 "if" "(" Expr ")" "->" GuardAction Newline ;

GuardAction    = "@continue"
               | "@break"
               | "return" Expr
               | "set" Ident "=" Expr ;
```

## Built-In Types

- Integers: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`
- Floats: `f32`, `f64`
- Other primitives: `bool`, `string`, `unit`
- Collections and wrappers: `Vector<T>`, `Option<T>`, `Result<T,E>`, `Box<T>`

## Built-In Functions

The compiler MVP installs a small standard signature table:

- `debug_i32(value::i32) -> unit`
- `debug_string(value::string) -> unit`
- `abs_i32(value::i32) -> i32`
- `is_even_i32(value::i32) -> bool`
- `max_i32(left::i32, right::i32) -> i32`
- `vector_len_i32(items::Vector<i32>) -> i64`
- `vector_i32_3(first::i32, second::i32, third::i32) -> Vector<i32>`

These are type-checked like normal function calls. Runtime lowering for them is part of the next backend milestone.

## Current Compiler Guarantees

- Lexical rejection of bare `{` / `}`.
- Lexical rejection of unknown `@tags`.
- Required `@intent`, at least one `@in`, one `@out`, and `---`.
- Required sticky typing for inputs, outputs, loop items, and `let`.
- Named function closure validation.
- Optional named loop closure validation.
- Cross-file function resolution in `jb.toml` project mode.
- Basic expression type checking for arithmetic, comparisons, equality, logic, calls, returns, assignments, and loop element types.
