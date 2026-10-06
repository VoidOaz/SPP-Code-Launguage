# SPP native C++17 backend

The normal SPP build compiles `native/spp_native.cpp` into a static library through `build.rs` and links it into the Rust runtime.

The exported ABI is intentionally tiny and stable:

- `spp_fast_sqrt`
- `spp_fast_sin`
- `spp_fast_cos`
- `spp_fast_pow`
- `spp_vec3_length`
- `spp_vec3_dot`

Keep the C ABI narrow. Higher-level SPP semantics remain owned by Rust.
