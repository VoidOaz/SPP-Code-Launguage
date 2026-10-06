# SPP 2.0 Beta

**SPP — Scriptable Performance Programming**

SPP is a general-purpose programming language designed around readable syntax, a fast Rust runtime, an optimized bytecode VM, and a C++17 native hot-path layer.

## Architecture

```text
.spp source
   │
   ├── Lexer (Rust)
   ├── Parser (Rust)
   ├── SPP AST (Rust)
   │
   ├── optimized bytecode VM (Rust) ── fast path
   │
   └── reference AST interpreter (Rust) ── compatibility fallback
                  │
                  └── native module calls → C++17 ABI layer
```

The VM is intentionally conservative: it only takes the fast path when a program is representable by the optimized instruction set. Programs using classes, object members, arrays, strings, foreach, or function calls currently use the reference interpreter so semantics remain stable.

## Improvements over 1.1

- Optimized stack-based bytecode VM for tight numeric/control-flow programs.
- Automatic VM → AST interpreter fallback for unsupported features.
- Rust/C++17 FFI is now a real build component, rather than a future-only source file.
- Native math calls use the C++ layer for `sqrt`, `sin`, `cos`, `pow`, and Spp3D vector length/dot primitives.
- Integer arithmetic now detects overflow instead of silently producing invalid results or panicking on edge cases.
- `range()` stops safely at integer overflow boundaries.
- Parser accepts both `fn f(int x)` and `fn f(x: int)` parameter forms and supports trailing commas in calls/arrays.
- `build` embeds the full local import graph, not just the root source file.
- `build` now respects an explicit output path instead of always forcing `build/`.
- VS Code extension uses argument-safe process spawning instead of shell command construction.
- The extension no longer performs regex-based import rewriting; the SPP compiler owns import resolution.

## CLI

```powershell
spp run examples\hello.spp
spp check examples\imports.spp
spp build examples\hello.spp
spp build dist\my_game.exe examples\hello.spp
spp benchmark examples\bench.spp 5
spp tokens examples\hello.spp
spp ast examples\hello.spp
spp modules
spp --version
```

## Build

A C++17 compiler is required because the native backend is built as part of the normal Rust build.

```powershell
cargo build --release
```

On Linux/macOS, `c++` + `ar` are used by `build.rs`. On Windows/MSVC, `cl.exe` + the MSVC librarian are used.

## VS Code

Install the supplied `.vsix`, then either build SPP in the workspace or set `spp.executablePath` to the compiler executable.

Commands:

- **SPP: Run Current File**
- **SPP: Check Current File**
- **SPP: Build Current File**
- **SPP: Benchmark Current File**
- **SPP: Open SPP Terminal**
- **SPP: Verify SPP Runtime**

The extension does not embed the old 1.0 executable. This avoids version skew between the editor tooling and the compiler source.

## Built-in modules

`Math`, `Time`, `Random`, `Spp3D`, `File`, and `OS` remain part of the runtime API. Module names are case-insensitive.

## Performance

The included `examples/bench.spp` is a tight integer loop. In SPP 2.0 it is eligible for the bytecode VM, avoiding repeated AST traversal and `HashMap`-based variable lookup inside the hot loop.

Benchmark results should always be reported with the CPU, OS, compiler profile, and run count. This repository does not make a universal “SPP is X times faster” claim without a controlled benchmark.
