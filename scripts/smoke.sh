#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$ROOT/target/release/spp}"
"$BIN" --version
"$BIN" check "$ROOT/examples/hello.spp"
"$BIN" check "$ROOT/examples/features.spp"
"$BIN" check "$ROOT/examples/imports.spp"
"$BIN" benchmark "$ROOT/examples/bench.spp" 2
