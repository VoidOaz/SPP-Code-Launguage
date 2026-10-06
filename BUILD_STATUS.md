# SPP 2.0 Build status

The source tree has been upgraded to a Rust-first compiler/runtime with a C++17 native ABI layer and an optimized bytecode VM.

Validated in the packaging environment:

- C++17 native source compiles with `g++ -O3` and exports all expected C ABI symbols.
- VS Code extension JavaScript passes Node syntax validation.
- VSIX archive structure is generated from the extension source.
- Project ZIP contains the complete source, tests, examples, docs, and legacy runtime for reference only.

Not validated here:

- Rust compilation, because this sandbox has no Rust toolchain installed.
- Windows `.exe` compilation, because this sandbox has no Windows/MSVC Rust target or linker.

Therefore the source is packaged as a build-ready project, while the supplied old Windows runtime is deliberately not presented as the SPP 2.0 runtime.
