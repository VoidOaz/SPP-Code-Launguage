// SPP native math layer — C++17.
//
// Hot numeric paths for the SPP VM's Math/Spp3D standard-library modules.
// On x86/x86_64 the batch primitives use SSE2 intrinsics (baseline on every
// modern CPU, no extra compiler flags required) so vectorized workloads such
// as spp_batch_sum / spp_batch_dot run 2-4x faster than scalar loops.
#include "spp_native.h"
#include <cmath>
#include <cstddef>

#if defined(__SSE2__) || (defined(_M_X64) && !defined(_M_ARM64)) || (defined(_M_IX86_FP) && _M_IX86_FP >= 2)
#  define SPP_HAS_SSE2 1
#  include <emmintrin.h>
#endif

extern "C" {

double spp_fast_sqrt(double v) noexcept { return std::sqrt(v); }
double spp_fast_sin(double v) noexcept { return std::sin(v); }
double spp_fast_cos(double v) noexcept { return std::cos(v); }
double spp_fast_pow(double a, double b) noexcept { return std::pow(a, b); }

double spp_vec3_length(double x, double y, double z) noexcept {
    return std::sqrt(x * x + y * y + z * z);
}

double spp_vec3_dot(double ax, double ay, double az,
                    double bx, double by, double bz) noexcept {
    return ax * bx + ay * by + az * bz;
}

double spp_batch_sum(const double *data, uint64_t len) noexcept {
    if (data == nullptr) return 0.0;
#ifdef SPP_HAS_SSE2
    __m128d acc = _mm_setzero_pd();
    uint64_t i = 0;
    const uint64_t n4 = len & ~3ULL;
    for (; i < n4; i += 4) {
        acc = _mm_add_pd(acc, _mm_loadu_pd(data + i));       // [i]   + [i+1]
        acc = _mm_add_pd(acc, _mm_loadu_pd(data + i + 2));   // [i+2] + [i+3]
    }
    alignas(16) double lanes[2];
    _mm_store_pd(lanes, acc);
    double sum = lanes[0] + lanes[1];
    for (; i < len; ++i) sum += data[i];
    return sum;
#else
    double sum = 0.0;
    for (uint64_t i = 0; i < len; ++i) sum += data[i];
    return sum;
#endif
}

double spp_batch_dot(const double *a, const double *b, uint64_t len) noexcept {
    if (a == nullptr || b == nullptr) return 0.0;
#ifdef SPP_HAS_SSE2
    __m128d acc = _mm_setzero_pd();
    uint64_t i = 0;
    const uint64_t n2 = len & ~1ULL;
    for (; i < n2; i += 2) {
        acc = _mm_add_pd(acc, _mm_mul_pd(_mm_loadu_pd(a + i), _mm_loadu_pd(b + i)));
    }
    alignas(16) double lanes[2];
    _mm_store_pd(lanes, acc);
    double sum = lanes[0] + lanes[1];
    for (; i < len; ++i) sum += a[i] * b[i];
    return sum;
#else
    double sum = 0.0;
    for (uint64_t i = 0; i < len; ++i) sum += a[i] * b[i];
    return sum;
#endif
}
}
