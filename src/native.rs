//! FFI bridge to the SPP native layer.
//!
//! The native layer is polyglot by design:
//!   * `native/spp_native.cpp` — C++17 math primitives (SSE2-vectorized batch ops)
//!   * `native/spp_native.c`   — C11 core primitives (FNV-1a hashing, version stamp)
//! Both are compiled by `build.rs` into a single static archive and linked here.

unsafe extern "C" {
    // C++17 math layer
    fn spp_fast_sqrt(v: f64) -> f64;
    fn spp_fast_sin(v: f64) -> f64;
    fn spp_fast_cos(v: f64) -> f64;
    fn spp_fast_pow(a: f64, b: f64) -> f64;
    fn spp_vec3_length(x: f64, y: f64, z: f64) -> f64;
    fn spp_vec3_dot(ax: f64, ay: f64, az: f64, bx: f64, by: f64, bz: f64) -> f64;
    fn spp_batch_sum(data: *const f64, len: u64) -> f64;
    fn spp_batch_dot(a: *const f64, b: *const f64, len: u64) -> f64;

    // C11 core
    fn spp_fnv1a64(data: *const u8, len: u64) -> u64;
    fn spp_native_version() -> u32;
}

pub fn sqrt(v: f64) -> f64 { unsafe { spp_fast_sqrt(v) } }
pub fn sin(v: f64) -> f64 { unsafe { spp_fast_sin(v) } }
pub fn cos(v: f64) -> f64 { unsafe { spp_fast_cos(v) } }
pub fn pow(a: f64, b: f64) -> f64 { unsafe { spp_fast_pow(a, b) } }
pub fn vec3_length(x: f64, y: f64, z: f64) -> f64 { unsafe { spp_vec3_length(x, y, z) } }
pub fn vec3_dot(ax: f64, ay: f64, az: f64, bx: f64, by: f64, bz: f64) -> f64 { unsafe { spp_vec3_dot(ax, ay, az, bx, by, bz) } }

/// SSE2-accelerated sum over a slice of doubles (C++17 layer).
pub fn batch_sum(data: &[f64]) -> f64 {
    if data.is_empty() { return 0.0; }
    unsafe { spp_batch_sum(data.as_ptr(), data.len() as u64) }
}

/// SSE2-accelerated dot product of two equal-length slices (C++17 layer).
pub fn batch_dot(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() != b.len() { return None; }
    if a.is_empty() { return Some(0.0); }
    Some(unsafe { spp_batch_dot(a.as_ptr(), b.as_ptr(), a.len() as u64) })
}

/// FNV-1a 64-bit hash computed by the C11 core layer.
pub fn fnv1a64(data: &[u8]) -> u64 {
    if data.is_empty() { return 0xcbf2_9ce4_8422_2325; }
    unsafe { spp_fnv1a64(data.as_ptr(), data.len() as u64) }
}

/// Encoded native ABI version (major/minor/patch/build), e.g. 0x0002_0100 = v2.1.0.
pub fn native_version() -> u32 { unsafe { spp_native_version() } }

/// Human-readable native version string, e.g. "2.1.0".
pub fn native_version_string() -> String {
    let v = native_version();
    format!("{}.{}.{}", (v >> 24) & 0xFF, (v >> 16) & 0xFF, (v >> 8) & 0xFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_hash_matches_reference_fnv1a() {
        // Reference FNV-1a 64-bit vectors.
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn cpp_batch_ops_match_scalar_reference() {
        let xs: Vec<f64> = (1..=1000).map(|i| i as f64 * 0.5).collect();
        let ys: Vec<f64> = (1..=1000).map(|i| (i % 7) as f64).collect();
        assert_eq!(batch_sum(&xs), xs.iter().sum::<f64>());
        let ref_dot: f64 = xs.iter().zip(&ys).map(|(a, b)| a * b).sum();
        assert_eq!(batch_dot(&xs, &ys), Some(ref_dot));
        assert_eq!(batch_dot(&xs, &ys[..3]), None);
    }

    #[test]
    fn native_version_is_stamped() {
        assert_eq!(native_version_string(), "2.1.0");
    }
}
