# SPP 2.0 Architecture

## Front-end

`src/lexer/` converts source text into positioned tokens. `src/parser/` turns those tokens into the SPP AST in `src/ast.rs`.

## Execution engines

`src/vm.rs` is a compact stack VM. It compiles a conservative subset of SPP to instructions with local-variable slots, jumps, arithmetic and comparisons. `runtime::run_program()` attempts this engine first.

`src/runtime/mod.rs` remains the semantic reference interpreter. Unsupported VM features intentionally fall back to it. This gives SPP a safe optimization boundary: VM support can grow incrementally without making every language feature depend on the VM.

## Native boundary

`src/native.rs` exposes a tiny Rust FFI surface backed by `native/spp_native.cpp`. The C++ layer is compiled by `build.rs` and linked as a static library. Only numeric primitives cross the ABI; ownership, errors, objects, arrays and language semantics remain in Rust.

## Packaging

`spp build` validates and walks the complete local `.spp` import graph. The executable payload contains every source file in the graph. Startup parses the embedded bundle in memory, ignores secondary `main()` functions exactly like normal imports, and executes the merged program.

This avoids the previous failure mode where a built executable contained only its root source while local imports remained external.

## Editor integration

The VS Code extension delegates compilation and execution to the real SPP compiler using `child_process.spawn()`. It does not transform SPP source or construct shell command strings, so import semantics and compiler diagnostics stay authoritative in the compiler.
