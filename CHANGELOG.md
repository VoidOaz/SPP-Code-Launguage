# Changelog

## 2.0.0 Beta

- Added optimized stack-based bytecode VM with automatic interpreter fallback.
- Added checked integer arithmetic and safer `range()` iteration.
- Added real Rust ↔ C++17 native FFI integration for selected math/vector paths.
- Fixed explicit build-output path handling.
- Standalone builds now embed the complete local source/import bundle.
- Parser parameter forms are less ambiguous and trailing commas are supported.
- VS Code process execution is argument-safe and does not depend on shell interpolation.
- The extension no longer rewrites/bundles `.spp` source using JavaScript regexes.
- Removed the legacy 1.0 runtime binary from the extension package.

## 1.1.0 Beta

- Added native built-in import modules: Math, Time, Random/random, Spp3D, File and OS.
- Added module introspection with `spp modules`.
- Added optional C++17 hot-path backend source.
- Reworked the VS Code extension to use a bundled Windows runtime workflow.

## 1.0.0 Beta

- Initial beta language/runtime.
