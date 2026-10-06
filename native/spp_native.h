#pragma once
/* Shared native ABI header for the SPP runtime.
 * Consumed by both the C11 core (spp_native.c) and the C++17 math layer
 * (spp_native.cpp); linked into Rust through src/native.rs FFI bindings. */
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- C++17 math layer (native/spp_native.cpp) ---- */
double spp_fast_sqrt(double v);
double spp_fast_sin(double v);
double spp_fast_cos(double v);
double spp_fast_pow(double a, double b);
double spp_vec3_length(double x, double y, double z);
double spp_vec3_dot(double ax, double ay, double az, double bx, double by, double bz);
double spp_batch_sum(const double *data, uint64_t len);
double spp_batch_dot(const double *a, const double *b, uint64_t len);

/* ---- C11 core (native/spp_native.c) ---- */
uint64_t spp_fnv1a64(const uint8_t *data, uint64_t len);
uint32_t spp_native_version(void);

#ifdef __cplusplus
}
#endif
