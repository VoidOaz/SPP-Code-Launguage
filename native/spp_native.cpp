#include "spp_native.h"
#include <cmath>

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
}
